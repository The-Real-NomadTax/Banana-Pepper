#!/bin/bash
# Builds apps/android/build/BananaPepper.apk without Gradle (Android SDK build-tools only).
# Inputs:
#   build/rootfs.tar.gz      from rootfs/make-rootfs.sh
#   build/proot/             from fetch-proot.sh
#   the i686 guest shim      built here with cargo (release)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../.."
SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
BT=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
PLATFORM=$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)/android.jar
VERSION_CODE=${VERSION_CODE:-$(git -C "$root" rev-list --count HEAD 2>/dev/null || echo 1)}
VERSION_NAME=${VERSION_NAME:-0.1.$VERSION_CODE}
B="$here/build"
for f in "$B/rootfs.tar.gz" "$B/proot/proot" "$B/proot/loader" "$B/proot/libtalloc.so.2" "$B/proot/libandroid-shmem.so"; do
    [[ -f $f ]] || { echo "missing $f (run rootfs/make-rootfs.sh and fetch-proot.sh)"; exit 1; }
done

(cd "$root" && cargo build -q --release -p banana-linux-shim --target i686-unknown-linux-gnu)

rm -rf "$B/apk" && mkdir -p "$B/apk/assets/runtime" "$B/apk/classes" "$B/apk/gen" "$B/apk/dex"
A="$B/apk/assets/runtime"
cp "$B/proot/"{proot,loader,libtalloc.so.2,libandroid-shmem.so} "$A/"
cp "$root/target/i686-unknown-linux-gnu/release/libarcade_linux_shim.so" "$A/"
cp "$here/rootfs/run-big-buck.sh" "$A/"
ln "$B/rootfs.tar.gz" "$A/rootfs.tar.gz" 2>/dev/null || cp "$B/rootfs.tar.gz" "$A/rootfs.tar.gz"
# Runtime version = rootfs content hash, so the app re-extracts only when it changes.
sha256sum "$B/rootfs.tar.gz" | cut -c1-16 > "$A/version.txt"

mkdir -p "$B/apk/src/com/bananapepper"
cat > "$B/apk/src/com/bananapepper/BuildInfo.java" <<JAVA
package com.bananapepper;
final class BuildInfo {
    static final int VERSION_CODE = $VERSION_CODE;
    static final String VERSION_NAME = "$VERSION_NAME";
    private BuildInfo() {}
}
JAVA

"$BT/aapt2" compile --dir "$here/app/res" -o "$B/apk/res.zip"
"$BT/aapt2" link -o "$B/apk/base.apk" -I "$PLATFORM" \
    --manifest "$here/app/AndroidManifest.xml" "$B/apk/res.zip" \
    $([[ ${RELEASE:-0} == 1 ]] || echo --debug-mode) --java "$B/apk/gen" -A "$B/apk/assets" -0 gz -0 so -0 2 \
    --min-sdk-version 26 --target-sdk-version 28 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME"
# Argument files keep paths with spaces intact.
find "$here/app/src" "$B/apk/src" "$B/apk/gen" -name '*.java' | sed 's/.*/"&"/' > "$B/apk/sources.txt"
javac -nowarn -Xlint:-options -source 11 -target 11 -encoding UTF-8 -classpath "$PLATFORM" -d "$B/apk/classes" \
    @"$B/apk/sources.txt"
find "$B/apk/classes" -name '*.class' > "$B/apk/classes.txt"
"$BT/d8" --min-api 26 --lib "$PLATFORM" --output "$B/apk/dex" @"$B/apk/classes.txt"
cp "$B/apk/base.apk" "$B/apk/unsigned.apk"
(cd "$B/apk/dex" && zip -q -0 "$B/apk/unsigned.apk" classes.dex)
"$BT/zipalign" -p -f 4 "$B/apk/unsigned.apk" "$B/apk/aligned.apk"
KS="$here/debug.keystore"
if [[ ! -f $KS ]]; then
    keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias bananapepper \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Banana pepper Debug" >/dev/null
fi
"$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
    --out "$B/BananaPepper.apk" "$B/apk/aligned.apk"
ls -la "$B/BananaPepper.apk"
