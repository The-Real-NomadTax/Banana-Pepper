#!/bin/bash
# Downloads proot (and its two libraries) for Android arm64 from the Termux
# package repository into apps/android/build/proot/.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/build/proot"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
repo=https://packages.termux.dev/apt/termux-main
curl -fsSL "$repo/dists/stable/main/binary-aarch64/Packages" -o "$work/Packages"
for pkg in proot libtalloc libandroid-shmem; do
    file=$(awk -v p="$pkg" '$1=="Package:"{cur=$2} $1=="Filename:" && cur==p {print $2; exit}' "$work/Packages")
    curl -fsSL "$repo/$file" -o "$work/$pkg.deb"
    dpkg-deb -x "$work/$pkg.deb" "$work/x"
done
P="$work/x/data/data/com.termux/files/usr"
mkdir -p "$out/include"
cp -R "$P/include/." "$out/include/"
cp "$P/bin/proot" "$P/libexec/proot/loader" "$out/"
cp "$P/lib/libtalloc.so."[0-9]*.[0-9]* "$out/libtalloc.so.2"
cp "$P/lib/libandroid-shmem.so" "$out/"
ls -la "$out"
