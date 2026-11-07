#!/bin/bash
# Native Kotlin frontend + shared Rust backend, using SDK tools without Gradle.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
BT=$(find "$SDK/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)
PLATFORM=$(find "$SDK/platforms" -name android.jar | sort -V | tail -1)
NDK=${ANDROID_NDK_HOME:-$(find "$SDK/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)}
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
B="$here/build"
if [[ -z ${KOTLINC:-} ]]; then
    if [[ -x "$B/kotlin-2.2.21/kotlinc/bin/kotlinc" ]]; then KOTLINC="$B/kotlin-2.2.21/kotlinc/bin/kotlinc";
    else KOTLINC=kotlinc; fi
fi
VERSION_CODE=${VERSION_CODE:-$(date +%s)}
VERSION_NAME=${VERSION_NAME:-0.2.0}
APP_ID=${APP_ID:-com.bananapepper}
for f in "$B/rootfs.tar.gz" "$B/proot/proot" "$B/proot/loader" "$B/proot/libtalloc.so.2" "$B/proot/libandroid-shmem.so"; do
    [[ -f $f ]] || { echo "missing $f (run rootfs/make-rootfs.sh and fetch-proot.sh)"; exit 1; }
done
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android26-clang"
export CC_aarch64_linux_android="$TC/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$TC/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$TC/llvm-ar"
export CXXFLAGS_aarch64_linux_android="-std=c++20"
(cd "$root" && cargo build --release -p banana-ffi --target aarch64-linux-android)
(cd "$root" && cargo build --release -p banana-linux-shim --target i686-unknown-linux-gnu)
(cd "$root" && cargo build --release -p banana-super-cars-hook -p banana-ford-hook --target i686-pc-windows-msvc)
rm -rf "$B/kotlin-apk"
mkdir -p "$B/kotlin-apk/"{assets/runtime/tools,gen,src/com/bananapepper,classes,dex,lib/arm64-v8a}
A="$B/kotlin-apk/assets/runtime"
cp "$B/proot/"{proot,loader,libtalloc.so.2,libandroid-shmem.so} "$A/"
cp "$root/target/i686-unknown-linux-gnu/release/libarcade_linux_shim.so" "$A/tools/"
cp "$root/target/i686-pc-windows-msvc/release/"{arcade_wine_hook.dll,inpout32.dll} "$A/tools/"
cp "$here/rootfs/run-game.sh" "$A/tools/"
if [[ -f "$B/box64" ]]; then cp "$B/box64" "$A/tools/box64"; fi
if [[ -d "$B/wine" ]]; then cp -rL "$B/wine" "$A/tools/wine"; fi
if [[ -d "$B/x64-libs" ]]; then cp -rL "$B/x64-libs" "$A/tools/"; fi
if [[ -d "$root/runtime/jurassic/libs" ]]; then
    mkdir -p "$A/tools/legacy-libs"
    for lib in libtheora.so.0 libPhysXLoader.so.1 libPhysXCore.so.1 libNxCooking.so.1; do
        [[ ! -f "$root/runtime/jurassic/libs/$lib" ]] || cp -L "$root/runtime/jurassic/libs/$lib" "$A/tools/legacy-libs/$lib"
    done
    for lib in libNxCooking libPhysXCore libPhysXLoader; do
        [[ ! -f "$A/tools/legacy-libs/$lib.so.1" ]] || cp "$A/tools/legacy-libs/$lib.so.1" "$A/tools/legacy-libs/$lib.so"
    done
fi
ln "$B/rootfs.tar.gz" "$A/rootfs.tar.gz" 2>/dev/null || cp "$B/rootfs.tar.gz" "$A/rootfs.tar.gz"
sha256sum "$B/rootfs.tar.gz" | cut -c1-16 > "$A/version.txt"
(cd "$A/tools" && find . -type f -print0 | sort -z | xargs -0 sha256sum) | sha256sum | cut -c1-16 > "$A/tools-version.txt"
cp "$root/target/aarch64-linux-android/release/libbanana_ffi.so" "$B/kotlin-apk/lib/arm64-v8a/"
# zarchive is C++; package the NDK runtime alongside the Rust JNI library.
cp "$NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$B/kotlin-apk/lib/arm64-v8a/"
"$TC/llvm-strip" --strip-unneeded "$B/kotlin-apk/lib/arm64-v8a/"*.so
cat > "$B/kotlin-apk/src/com/bananapepper/BuildInfo.kt" <<KOTLIN
package com.bananapepper
object BuildInfo { const val VERSION_CODE = $VERSION_CODE; const val VERSION_NAME = "$VERSION_NAME" }
KOTLIN
"$BT/aapt2" compile --dir "$here/app/res" -o "$B/kotlin-apk/res.zip"
"$BT/aapt2" link -o "$B/kotlin-apk/base.apk" -I "$PLATFORM" \
    --manifest "$here/app/AndroidManifest.xml" "$B/kotlin-apk/res.zip" \
    --rename-manifest-package "$APP_ID" \
    $([[ ${RELEASE:-0} == 1 ]] || echo --debug-mode) --java "$B/kotlin-apk/gen" \
    -A "$B/kotlin-apk/assets" -0 gz -0 so -0 2 --min-sdk-version 26 --target-sdk-version 28 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME"
find "$here/app/src" "$B/kotlin-apk/src" -name '*.kt' | sed 's/.*/"&"/' > "$B/kotlin-apk/sources.txt"
"$KOTLINC" @"$B/kotlin-apk/sources.txt" -classpath "$PLATFORM" -jvm-target 1.8 -no-reflect -include-runtime -d "$B/kotlin-apk/app.jar"
"$BT/d8" --min-api 26 --lib "$PLATFORM" --output "$B/kotlin-apk/dex" "$B/kotlin-apk/app.jar"
cp "$B/kotlin-apk/base.apk" "$B/kotlin-apk/unsigned.apk"
(cd "$B/kotlin-apk" && zip -q -r -0 unsigned.apk lib)
(cd "$B/kotlin-apk/dex" && zip -q -0 "$B/kotlin-apk/unsigned.apk" classes*.dex)
"$BT/zipalign" -P 16 -f 4 "$B/kotlin-apk/unsigned.apk" "$B/kotlin-apk/aligned.apk"
KS="$here/debug.keystore"
if [[ ! -f $KS ]]; then
    keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias bananapepper \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Banana Pepper Debug" >/dev/null
fi
"$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
    --out "$B/BananaPepper.apk" "$B/kotlin-apk/aligned.apk"
"$BT/apksigner" verify "$B/BananaPepper.apk"
ls -lh "$B/BananaPepper.apk"
