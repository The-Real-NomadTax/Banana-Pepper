#!/bin/bash
# Prepare ignored desktop dependencies in this checkout, without installing packages.
set -euo pipefail

if [[ $(uname -s) != Linux ]]; then
    exit 0
fi

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
common_git=$(git -C "$project_root" rev-parse --path-format=absolute --git-common-dir)
primary_root=$(cd -- "$common_git/.." && pwd)
runtime="$project_root/runtime"
xvfb="$runtime/tools/xvfb/usr/bin/Xvfb"

# The existing Jurassic launcher checks this local path even in desktop mode.
if [[ ! -x "$xvfb" ]]; then
    source_xvfb=$(command -v Xvfb || true)
    if [[ -z "$source_xvfb" && "$primary_root" != "$project_root" && -x "$primary_root/runtime/tools/xvfb/usr/bin/Xvfb" ]]; then
        source_xvfb="$primary_root/runtime/tools/xvfb/usr/bin/Xvfb"
    fi
    if [[ -z "$source_xvfb" ]]; then
        echo "Xvfb is required by the current launchers. Install your distribution's Xvfb package, then rerun just build." >&2
        exit 1
    fi
    mkdir -p -- "$(dirname -- "$xvfb")"
    cp -- "$source_xvfb" "$xvfb"
    chmod u+x "$xvfb"
fi

# Reuse only dependency archives from the primary checkout, never game files,
# logs or saves. The existing setup script verifies the archives' pinned hashes.
cache="$runtime/tools/jurassic"
mkdir -p -- "$cache"
for archive in physx.tar.gz theora.pkg.tar.zst; do
    source_archive="$primary_root/runtime/tools/jurassic/$archive"
    if [[ ! -f "$cache/$archive" && -f "$source_archive" ]]; then
        cp -- "$source_archive" "$cache/$archive"
    fi
done

libs="$runtime/jurassic/libs"
if [[ ! -f "$libs/libPhysXLoader.so.1" || ! -f "$libs/libPhysXCore.so.1" || ! -f "$libs/libNxCooking.so.1" || ! -f "$libs/libtheora.so.0" ]]; then
    bash "$project_root/scripts/setup-jurassic-libs.sh"
fi

printf 'Desktop runtime dependencies ready in %s\n' "$runtime"
