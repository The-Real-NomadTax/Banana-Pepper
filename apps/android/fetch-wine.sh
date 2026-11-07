#!/bin/bash
# Pinned x86-64 WoW64 build: 32-bit Windows guests need no Android AArch32 ABI.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
B="$here/build"
archive="$B/wine-11.0-amd64-wow64.tar.xz"
checksum=39574efa1132c3ca0d5c77dd2eddbe4a49cca0d6cc2c290ff4924493a1c40314
mkdir -p "$B"
if [[ ! -f "$archive" ]]; then
    curl --fail --location --retry 2 \
        https://github.com/Kron4ek/Wine-Builds/releases/download/11.0/wine-11.0-amd64-wow64.tar.xz \
        -o "$archive.downloading"
    mv "$archive.downloading" "$archive"
fi
printf '%s  %s\n' "$checksum" "$archive" | sha256sum -c -
if [[ ! -f "$B/wine/.banana-version" ]] || [[ $(cat "$B/wine/.banana-version") != "$checksum" ]]; then
    rm -rf "$B/wine.installing"
    mkdir -p "$B/wine.installing"
    tar -xJf "$archive" -C "$B/wine.installing" --strip-components=1
    printf '%s\n' "$checksum" > "$B/wine.installing/.banana-version"
    rm -rf "$B/wine"
    mv "$B/wine.installing" "$B/wine"
fi
