#!/bin/sh
# Rebuild the ELF32 fixtures for Water's dynamic linker test (tests/elf32.rs)
# on a Linux host with a 32-bit C toolchain (gcc -m32). The binaries are
# committed so the Windows test needs no Linux toolchain.
set -eu
cd "$(dirname "$0")"
mkdir -p game/lib
flags="-m32 -O1 -fno-stack-protector -fcf-protection=none"
gcc $flags -fPIC -shared -Wl,-soname,libbanana_fixture.so -o game/lib/libbanana_fixture.so src/library.c
gcc $flags -fPIC -shared -Wl,-soname,libbanana_plugin.so -Wl,-rpath,'$ORIGIN' \
    -o game/lib/libbanana_plugin.so src/plugin.c game/lib/libbanana_fixture.so
gcc $flags -fno-pie -no-pie -Wl,-rpath,'$ORIGIN/lib' -o game/program src/program.c \
    -Lgame/lib -lbanana_fixture -ldl
strip --strip-unneeded game/program game/lib/*.so
