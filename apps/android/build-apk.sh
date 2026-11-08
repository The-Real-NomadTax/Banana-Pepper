#!/bin/bash
# Native Kotlin frontend + shared Rust backend, using SDK tools without Gradle.
# Runs on Linux, or on Windows in Git Bash with the Windows SDK and NDK. The
# PRoot tracer and the title adapters (build-proot.sh, banana-dist
# android-resources) need a Linux host; on other hosts build them there first.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
case "$(uname -s)" in
    Linux*) host=linux-x86_64 exe= cmd= ;;
    MINGW* | MSYS* | CYGWIN*) host=windows-x86_64 exe=.exe cmd=.cmd ;;
    Darwin*) host=darwin-x86_64 exe= cmd= ;;
    *) echo "Unsupported build host $(uname -s)"; exit 1 ;;
esac
default_sdk=$HOME/Android/Sdk
[[ $host == windows-x86_64 ]] && default_sdk=${LOCALAPPDATA:-}/Android/Sdk
SDK=${ANDROID_HOME:-$default_sdk}
BT=$(find "$SDK/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)
PLATFORM=$(find "$SDK/platforms" -name android.jar | sort -V | tail -1)
NDK=${ANDROID_NDK_HOME:-$(find "$SDK/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)}
TC="$NDK/toolchains/llvm/prebuilt/$host/bin"
B="$here/build"
KOTLIN=${KOTLIN_HOME:-$B/kotlin-2.2.21/kotlinc}
PYTHON=$(command -v python3 || command -v python)
VERSION_CODE=${VERSION_CODE:-$(date +%s)}
VERSION_NAME=${VERSION_NAME:-0.2.0}
APP_ID=${APP_ID:-com.bananapepper}
# Paths handed to Java tools inside argument files must be native.
native() { if command -v cygpath >/dev/null; then cygpath -m "$1"; else printf '%s\n' "$1"; fi; }
resources="$root/target/dist/banana-pepper-android-resources"
if [[ $host == linux-x86_64 ]]; then
    (cd "$root" && cargo run --release -p banana-dist --locked -- android-resources)
    "$here/build-proot.sh"
fi
for f in "$B/rootfs.tar.gz" "$B/proot/proot" "$B/proot/loader" "$B/proot/libtalloc.so.2" \
    "$B/proot/libandroid-shmem.so" "$resources/banana-pepper.dist" "$KOTLIN/lib/kotlin-compiler.jar"; do
    [[ -f $f ]] || { echo "missing $f (see docs/android.md: Build)"; exit 1; }
done
grep -q BANANA_VFS_PROTOCOL_2 "$B/proot/proot" || { echo "build/proot/proot lacks the view protocol; run build-proot.sh"; exit 1; }
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android26-clang$cmd"
export CC_aarch64_linux_android="$TC/aarch64-linux-android26-clang$cmd"
export CXX_aarch64_linux_android="$TC/aarch64-linux-android26-clang++$cmd"
export AR_aarch64_linux_android="$TC/llvm-ar$exe"
export CXXFLAGS_aarch64_linux_android="-std=c++20"
(cd "$root" && cargo build --release --locked -p banana-ffi --target aarch64-linux-android)
rm -rf "$B/kotlin-apk"
mkdir -p "$B/kotlin-apk/"{assets/runtime/tools,gen,src/com/bananapepper,classes,dex,lib/arm64-v8a}
A="$B/kotlin-apk/assets/runtime"
cp "$B/proot/"{proot,loader,libtalloc.so.2,libandroid-shmem.so} "$A/"
cp -R "$resources/games" "$A/tools/"
cp "$resources/"{banana-pepper.dist,artifacts.json,THIRD-PARTY-NOTICES.txt} "$A/tools/"
if [[ -f "$B/box64" ]]; then cp "$B/box64" "$A/tools/box64"; fi
if [[ -d "$B/wine" ]]; then cp -rL "$B/wine" "$A/tools/wine"; fi
if [[ -d "$B/x64-libs" ]]; then cp -rL "$B/x64-libs" "$A/tools/"; fi
# Optional redistributable ABI dependencies use per-user storage, never game/runtime trees.
legacy=${BANANA_ANDROID_LEGACY32:-${XDG_DATA_HOME:-$HOME/.local/share}/banana-pepper/dependencies/legacy32}
mkdir -p "$A/tools/dependencies/legacy32"
if [[ -d "$legacy" ]]; then cp -rL "$legacy/." "$A/tools/dependencies/legacy32/"; fi
ln "$B/rootfs.tar.gz" "$A/rootfs.tar.gz" 2>/dev/null || cp "$B/rootfs.tar.gz" "$A/rootfs.tar.gz"
sha256sum "$B/rootfs.tar.gz" | cut -c1-16 > "$A/version.txt"
(cd "$A/tools" && find . -type f -print0 | sort -z | xargs -0 sha256sum) | sha256sum | cut -c1-16 > "$A/tools-version.txt"
cp "$root/target/aarch64-linux-android/release/libbanana_ffi.so" "$B/kotlin-apk/lib/arm64-v8a/"
# zarchive is C++; package the NDK runtime alongside the Rust JNI library.
cp "$NDK/toolchains/llvm/prebuilt/$host/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$B/kotlin-apk/lib/arm64-v8a/"
"$TC/llvm-strip$exe" --strip-unneeded "$B/kotlin-apk/lib/arm64-v8a/"*.so
cat > "$B/kotlin-apk/src/com/bananapepper/BuildInfo.kt" <<KOTLIN
package com.bananapepper
object BuildInfo { const val VERSION_CODE = $VERSION_CODE; const val VERSION_NAME = "$VERSION_NAME" }
KOTLIN
"$BT/aapt2$exe" compile --dir "$here/app/res" -o "$B/kotlin-apk/res.zip"
"$BT/aapt2$exe" link -o "$B/kotlin-apk/base.apk" -I "$PLATFORM" \
    --manifest "$here/app/AndroidManifest.xml" "$B/kotlin-apk/res.zip" \
    --rename-manifest-package "$APP_ID" \
    $([[ ${RELEASE:-0} == 1 ]] || echo --debug-mode) --java "$B/kotlin-apk/gen" \
    -A "$B/kotlin-apk/assets" -0 gz -0 so -0 2 --min-sdk-version 26 --target-sdk-version 28 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME"
find "$here/app/src" "$B/kotlin-apk/src" -name '*.kt' | while read -r f; do printf '"%s"\n' "$(native "$f")"; done \
    > "$B/kotlin-apk/sources.txt"
java -Xmx2g -cp "$(native "$KOTLIN/lib/kotlin-compiler.jar")" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -kotlin-home "$(native "$KOTLIN")" @"$(native "$B/kotlin-apk/sources.txt")" \
    -classpath "$(native "$PLATFORM")" -jvm-target 1.8 -no-reflect -include-runtime \
    -d "$(native "$B/kotlin-apk/app.jar")"
java -cp "$(native "$BT/lib/d8.jar")" com.android.tools.r8.D8 --min-api 26 \
    --lib "$(native "$PLATFORM")" --output "$(native "$B/kotlin-apk/dex")" "$(native "$B/kotlin-apk/app.jar")"
cp "$B/kotlin-apk/base.apk" "$B/kotlin-apk/unsigned.apk"
# Native libraries and dex files go in uncompressed (stored).
"$PYTHON" - "$(native "$B/kotlin-apk")" <<'PY'
import os, sys, zipfile
base = sys.argv[1]
with zipfile.ZipFile(os.path.join(base, "unsigned.apk"), "a", zipfile.ZIP_STORED) as apk:
    for folder in ("lib",):
        for top, _, files in os.walk(os.path.join(base, folder)):
            for name in sorted(files):
                path = os.path.join(top, name)
                apk.write(path, os.path.relpath(path, base).replace(os.sep, "/"))
    for name in sorted(os.listdir(os.path.join(base, "dex"))):
        if name.startswith("classes") and name.endswith(".dex"):
            apk.write(os.path.join(base, "dex", name), name)
PY
"$BT/zipalign$exe" -P 16 -f 4 "$B/kotlin-apk/unsigned.apk" "$B/kotlin-apk/aligned.apk"
KS="$here/debug.keystore"
if [[ ! -f $KS ]]; then
    keytool -genkeypair -keystore "$(native "$KS")" -storepass android -keypass android -alias bananapepper \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Banana Pepper Debug" >/dev/null
fi
java -jar "$(native "$BT/lib/apksigner.jar")" sign --ks "$(native "$KS")" --ks-pass pass:android --key-pass pass:android \
    --out "$(native "$B/BananaPepper.apk")" "$(native "$B/kotlin-apk/aligned.apk")"
java -jar "$(native "$BT/lib/apksigner.jar")" verify "$(native "$B/BananaPepper.apk")"
ls -lh "$B/BananaPepper.apk"
