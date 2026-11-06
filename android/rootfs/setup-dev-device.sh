#!/bin/bash
# One-time build environment on a connected arm64 Android phone (adb), under
# /data/local/tmp/af: proot + Debian trixie arm64, build tools, and box64
# (BOX32 enabled, patched for section-less i386 executables). The phone builds
# box64 natively, so no cross toolchain or QEMU is needed on the host.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
android="$here/.."
AF=/data/local/tmp/af
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
[[ -f $android/build/proot/proot ]] || "$android/fetch-proot.sh"
rootfs_url=https://github.com/termux/proot-distro/releases/download/v4.29.0/debian-trixie-aarch64-pd-v4.29.0.tar.xz
curl -fsSL "$rootfs_url" -o "$work/debian.tar.xz"
xz -d "$work/debian.tar.xz"      # toybox tar on the phone cannot run xz
adb shell "mkdir -p $AF/bin $AF/tmp $AF/share $AF/rootfs"
adb push "$android/build/proot/"* $AF/bin/ >/dev/null
adb push "$work/debian.tar" $AF/debian-trixie-aarch64-pd-v4.29.0.tar >/dev/null
adb push "$here/device-enter.sh" $AF/enter.sh >/dev/null
adb push "$here/patches/box64-sectionless-elf32.patch" $AF/share/ >/dev/null
adb shell "cd $AF && chmod 755 bin/* enter.sh && tar -xf debian-trixie-aarch64-pd-v4.29.0.tar -C rootfs 2>/dev/null; true"
adb shell "cd $AF && ./enter.sh /bin/sh -c '
  set -e
  echo nameserver 8.8.8.8 > /etc/resolv.conf
  apt-get update -q
  DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
      build-essential cmake git python3 ca-certificates patch
  cd /root && rm -rf box64 && git clone -q https://github.com/ptitSeb/box64 && cd box64
  git checkout -q 59b3eb1555406db697941ddd5e414c7085b90e4f
  patch -p1 < /af/box64-sectionless-elf32.patch
  mkdir -p build && cd build
  cmake .. -DARM_DYNAREC=ON -DBOX32=ON -DBOX32_BINFMT=OFF -DCMAKE_BUILD_TYPE=RelWithDebInfo >/dev/null
  make -j8 --jobserver-style=pipe >/dev/null   # SELinux denies named FIFOs here
  echo BOX64_BUILT'"
