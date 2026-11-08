#!/bin/bash
# The x86-64 C++ and GCC runtime libraries Box64 needs for Wine's 64-bit side,
# taken from Debian trixie amd64 to match the runtime rootfs's C library (not
# from the build host, whose newer glibc symbols Box64 could not satisfy).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/build/x64-libs"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mirror=https://deb.debian.org/debian
curl -fsSL "$mirror/dists/trixie/main/binary-amd64/Packages.xz" | xz -d > "$work/Packages"
for pkg in libstdc++6 libgcc-s1; do
    file=$(awk -v p="$pkg" '$1=="Package:"{cur=$2} $1=="Filename:" && cur==p {print $2; exit}' "$work/Packages")
    sum=$(awk -v p="$pkg" '$1=="Package:"{cur=$2} $1=="SHA256:" && cur==p {print $2; exit}' "$work/Packages")
    [[ -n $file && -n $sum ]] || { echo "Debian has no $pkg"; exit 1; }
    curl -fsSL "$mirror/$file" -o "$work/$pkg.deb"
    printf '%s  %s\n' "$sum" "$work/$pkg.deb" | sha256sum -c - >/dev/null
    dpkg-deb -x "$work/$pkg.deb" "$work/x"
done
rm -rf "$out.installing"
mkdir -p "$out.installing/licenses"
for name in libstdc++.so.6 libgcc_s.so.1; do
    source=$(find "$work/x" -name "$name" | head -1)
    [[ -n $source ]] || { echo "Missing $name"; exit 1; }
    readelf -h "$source" | grep -q 'Advanced Micro Devices X86-64' || { echo "$source is not x86-64"; exit 1; }
    cp -L "$source" "$out.installing/$name"
done
# Their copyright files live in the GCC base package their doc folders link to.
for pkg in libstdc++6 libgcc-s1; do
    doc="$work/x/usr/share/doc/$pkg"
    if [[ -L $doc && ! -f $doc/copyright ]]; then
        base=$(basename "$(readlink "$doc")")
        file=$(awk -v p="$base" '$1=="Package:"{cur=$2} $1=="Filename:" && cur==p {print $2; exit}' "$work/Packages")
        curl -fsSL "$mirror/$file" -o "$work/$base.deb"
        dpkg-deb -x "$work/$base.deb" "$work/x"
    fi
    cp -L "$doc/copyright" "$out.installing/licenses/$pkg.copyright"
done
rm -rf "$out"
mv "$out.installing" "$out"
ls -la "$out"
