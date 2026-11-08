#!/bin/bash
# Builds the Android runtime rootfs and Box64 on an x86-64 Linux host (or WSL)
# without a phone: Debian trixie arm64 runs under PRoot with QEMU user-mode
# emulation. Same outputs as make-rootfs.sh: apps/android/build/rootfs.tar.gz
# and apps/android/build/box64.
#
# Needs qemu-aarch64 (qemu-user), proot (PROOT=... or on PATH), curl, xz and
# GNU tar. Work happens under BANANA_ROOTFS_WORK (default ~/.cache/...), which
# must be a native Linux filesystem (not /mnt/c on WSL).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../build"
work=${BANANA_ROOTFS_WORK:-$HOME/.cache/banana-pepper/android-rootfs}
PROOT=${PROOT:-proot}
QEMU=${QEMU:-$(command -v qemu-aarch64)}
jobs=${JOBS:-$(nproc)}
box64_commit=59b3eb1555406db697941ddd5e414c7085b90e4f
url=https://github.com/termux/proot-distro/releases/download/v4.29.0/debian-trixie-aarch64-pd-v4.29.0.tar.xz
checksum=${ROOTFS_SHA256:-3834a11cbc6496935760bdc20cca7e2c25724d0cd8f5e4926da8fd5ca1857918}
mkdir -p "$out" "$work/share" "$work/tmp"
tarball="$work/debian-trixie-aarch64.tar.xz"
if [[ ! -f $tarball ]]; then
    curl -fL --retry 2 "$url" -o "$tarball.downloading"
    mv "$tarball.downloading" "$tarball"
fi
if [[ -n $checksum ]]; then printf '%s  %s\n' "$checksum" "$tarball" | sha256sum -c -; fi

# A fresh Debian tree at $1.
fresh() {
    rm -rf "$1"
    mkdir -p "$1"
    tar --no-same-owner -xJf "$tarball" -C "$1" --strip-components=1 2>/dev/null || true
    [[ -x $1/bin/bash ]] || { echo "Cannot unpack $tarball"; exit 1; }
}
# Run a command as (fake) root inside the arm64 tree at $1.
enter() {
    local root=$1
    shift
    # PRoot's seccomp acceleration fails with QEMU on some kernels (WSL).
    PROOT_NO_SECCOMP=1 PROOT_TMP_DIR="$work/tmp" "$PROOT" -q "$QEMU" -0 -r "$root" \
        -b /dev -b /proc -b /sys -b "$work/share:/af" -w /root \
        /usr/bin/env -i HOME=/root TERM=xterm LANG=C.UTF-8 \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin "$@"
}

# 1. Box64 (BOX32 enabled, Banana Pepper patches) in a build tree.
if [[ ! -x $work/share/box64-install/usr/local/bin/box64 ]]; then
    fresh "$work/dev"
    cp "$here/patches/"*.patch "$work/share/"
    enter "$work/dev" /bin/sh -c "
        set -e
        echo nameserver 8.8.8.8 > /etc/resolv.conf
        apt-get update -q
        DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
            build-essential cmake git python3 ca-certificates patch
        cd /root && rm -rf box64 && git clone -q https://github.com/ptitSeb/box64 && cd box64
        git checkout -q $box64_commit
        patch -p1 < /af/box64-sectionless-elf32.patch
        patch -p1 < /af/box64-android-guest-exec.patch
        patch -p1 < /af/box32-pthread-attribute-width.patch
        mkdir -p build && cd build
        cmake .. -DARM_DYNAREC=ON -DBOX32=ON -DBOX32_BINFMT=OFF -DCMAKE_BUILD_TYPE=RelWithDebInfo >/dev/null
        make -j$jobs >/dev/null
        rm -rf /af/box64-install && make install DESTDIR=/af/box64-install >/dev/null
        strip --strip-debug /af/box64-install/usr/local/bin/box64
        echo BOX64_BUILT"
fi

# 2. The release rootfs: only what the Android runtime needs.
cp "$here/afbridge.c" "$here/afaudio.c" "$here/build-rootfs-guest.sh" "$here/trim-rootfs-guest.sh" "$work/share/"
fresh "$work/release"
enter "$work/release" /bin/bash /af/build-rootfs-guest.sh

# 3. Pack it as the app unpacks it.
tar --numeric-owner --owner=0 --group=0 \
    --exclude='./dev/*' --exclude='./proc/*' --exclude='./sys/*' --exclude='./tmp/*' \
    -C "$work/release" -czf "$out/rootfs.tar.gz.building" .
mv "$out/rootfs.tar.gz.building" "$out/rootfs.tar.gz"
cp "$work/share/box64-install/usr/local/bin/box64" "$out/box64"
ls -la "$out/rootfs.tar.gz" "$out/box64"
