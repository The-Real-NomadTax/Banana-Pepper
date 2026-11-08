#!/bin/bash
# Build the pinned PRoot tracer with the shared-filesystem protocol extension.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
commit=52d65a4dc140ca554d241a1438ddd4f6b00c9813
out="$here/build/proot"
source="$here/build/proot-src"
SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
NDK=${ANDROID_NDK_HOME:-$(find "$SDK/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)}
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
if [[ -x "$TC/aarch64-linux-android26-clang" ]]; then
    CC="$TC/aarch64-linux-android26-clang"
    STRIP="$TC/llvm-strip"
    tools=()
else
    # An NDK for another host (e.g. the Windows NDK seen from WSL): its sysroot
    # and compiler runtime are host-independent, so the host's clang and lld
    # (LLD_DIR, e.g. Rust's gcc-ld directory) build the same binary.
    prebuilt=$(find "$NDK/toolchains/llvm/prebuilt" -mindepth 1 -maxdepth 1 -type d | head -1)
    resource=$(find "$prebuilt/lib/clang" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)
    CC="clang --target=aarch64-linux-android26 --sysroot=$prebuilt/sysroot -resource-dir=$resource"
    CC+=" --rtlib=compiler-rt --unwindlib=libunwind -fuse-ld=lld${LLD_DIR:+ -B$LLD_DIR}"
    # The loader is embedded with binutils that know AArch64.
    STRIP=${STRIP:-aarch64-linux-gnu-strip}
    tools=(STRIP="$STRIP" OBJCOPY=aarch64-linux-gnu-objcopy OBJDUMP=aarch64-linux-gnu-objdump)
fi
if [[ ! -d "$source/.git" ]]; then git clone https://github.com/termux/proot.git "$source"; fi
git -C "$source" fetch origin "$commit"
git -C "$source" checkout --detach "$commit"
python3 "$root/backends/android/proot/patch.py" "$source"
# fetch-proot supplies the matching Termux talloc/shmem libraries and headers.
[[ -f "$out/include/talloc.h" ]] || { echo 'Run fetch-proot.sh first'; exit 1; }
make -C "$source/src" -j"$(nproc)" \
    CC="$CC" \
    CPPFLAGS="-D_FILE_OFFSET_BITS=64 -D_GNU_SOURCE -I. -I$source/src -I$out/include" \
    LDFLAGS="-L$out -l:libtalloc.so.2 -landroid-shmem -Wl,-z,noexecstack -Wl,-z,max-page-size=16384" \
    PROOT_WITH_LIBANDROID_SHMEM=1 "${tools[@]}"
cp "$source/src/proot" "$out/proot"
cp "$source/src/loader/loader" "$out/loader"
"$STRIP" --strip-unneeded "$out/proot"
