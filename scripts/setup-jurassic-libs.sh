#!/bin/bash
# Stage dependencies locally. Does not install packages or modify game assets.
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cache="$project_root/runtime/tools/jurassic"
libs="$project_root/runtime/jurassic/libs"
mkdir -p "$cache" "$libs" "$cache/physx"
cd "$cache"
if [[ ! -f physx.tar.gz ]]; then
    curl --fail --location --max-time 60 'https://developer.download.nvidia.com/PhysX/2.8.1/PhysX_2.8.1_SDK_CoreLinux_deb.tar.gz' -o physx.tar.gz.download
    mv physx.tar.gz.download physx.tar.gz
fi
if [[ ! -f theora.pkg.tar.zst ]]; then
    curl --fail --location --max-time 30 'https://archive.archlinux.org/packages/l/lib32-libtheora/lib32-libtheora-1.1.1-13-x86_64.pkg.tar.zst' -o theora.pkg.tar.zst.download
    mv theora.pkg.tar.zst.download theora.pkg.tar.zst
fi
sha256sum --check <<'HASHES'
2a4822f6f5ba9c00993e529392c6fd957bb0c5e07518e9a08bae9ee502428913  physx.tar.gz
966f9063faee25ca02f8f7c92989c9f371eea8d526cea9923326f782778248e5  theora.pkg.tar.zst
HASHES
tar --no-same-owner --no-same-permissions -xf physx.tar.gz libphysx-2.8.1_4_i386.deb libphysx-common_2.8.1-4_i386.deb
for package in libphysx-2.8.1_4_i386.deb libphysx-common_2.8.1-4_i386.deb; do
    ar p "$package" data.tar.gz | tar --no-same-owner --no-same-permissions -xz -C "$cache/physx"
done
tar --no-same-owner --no-same-permissions -xf theora.pkg.tar.zst -C "$cache"
cp -a "$cache"/usr/lib32/libtheora* "$libs/"
cp "$cache/physx/usr/lib/libPhysXLoader.so.1" "$libs/"
cp "$cache/physx/usr/lib/PhysX/v2.8.1/"*.so.1 "$libs/"
printf 'Staged Jurassic Park libraries under %s\n' "$libs"
