#!/bin/bash
# Builds the Android runtime rootfs ON a connected arm64 phone (over adb) and
# pulls it to apps/android/build/rootfs.tar.gz. Requires the dev environment from
# setup-dev-device.sh (proot + Debian + box64 source/build in /data/local/tmp/af).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../build"
mkdir -p "$out"
AF=/data/local/tmp/af
adb push "$here/afbridge.c" "$here/afaudio.c" "$here/run-big-buck.sh" "$here/build-rootfs-guest.sh" "$here/trim-rootfs-guest.sh" $AF/share/ >/dev/null
# box64 install tree from the dev build (patched, BOX32 enabled).
adb shell "cd $AF && rm -rf share/box64-install && ./enter.sh /bin/sh -c 'cd /root/box64/build && make install DESTDIR=/af/box64-install >/dev/null && strip --strip-debug /af/box64-install/usr/local/bin/box64'"
if [[ ${TRIM_ONLY:-0} == 1 ]]; then
adb shell "cd $AF && ./enter-release.sh /bin/bash /af/trim-rootfs-guest.sh" 2>&1 | tail -2
else
adb shell "cd $AF && rm -rf release && mkdir -p release/rootfs && tar -xf debian-trixie-aarch64-pd-v4.29.0.tar -C release/rootfs 2>/dev/null; true"
adb shell "cd $AF && sed 's#R=\$AF/rootfs/debian-trixie-aarch64#R=\$AF/release/rootfs/debian-trixie-aarch64#' enter.sh > enter-release.sh && chmod 755 enter-release.sh"
adb shell "cd $AF && ./enter-release.sh /bin/bash /af/build-rootfs-guest.sh" 2>&1 | tail -3
fi
# Pack with GNU tar from the dev rootfs.
adb shell "cd $AF && rm -f share/rootfs.tar.gz && ./enter.sh /bin/sh -c 'mkdir -p /mnt/rel' && \
  LD_LIBRARY_PATH=bin PROOT_LOADER=bin/loader PROOT_TMP_DIR=tmp bin/proot -0 -r rootfs/debian-trixie-aarch64 \
  -b release/rootfs/debian-trixie-aarch64:/mnt/rel -b share:/af /usr/bin/env -i PATH=/usr/bin:/bin \
  tar --numeric-owner --exclude=./dev/* --exclude=./proc/* --exclude=./sys/* --exclude=./tmp/* -C /mnt/rel -czf /af/rootfs.tar.gz ."
adb pull $AF/share/rootfs.tar.gz "$out/rootfs.tar.gz"
adb pull $AF/share/box64-install/usr/local/bin/box64 "$out/box64"
ls -la "$out/rootfs.tar.gz"
