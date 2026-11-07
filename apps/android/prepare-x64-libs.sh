#!/bin/bash
# Collect the ELF64 runtime libraries needed by box64 guests on this x86-64 host.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
staged="$here/build/x64-libs.installing"
mkdir -p "$here/build"
rm -rf "$staged"
mkdir -p "$staged"
for name in libstdc++.so.6 libgcc_s.so.1; do
    source=
    for directory in /usr/lib /usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu; do
        if [[ -f "$directory/$name" ]]; then source="$directory/$name"; break; fi
    done
    [[ -n $source ]] || { echo "Missing host library $name; install the GCC runtimes first."; exit 1; }
    readelf -h "$source" | grep -q 'Advanced Micro Devices X86-64' || { echo "$source is not ELF64 x86-64"; exit 1; }
    cp -L "$source" "$staged/$name"
done
for package in gcc-libs; do
    if [[ -d /usr/share/licenses/$package ]]; then
        mkdir -p "$staged/licenses"
        cp -r "/usr/share/licenses/$package" "$staged/licenses/"
    fi
done
rm -rf "$here/build/x64-libs"
mv "$staged" "$here/build/x64-libs"
