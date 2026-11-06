#!/usr/bin/env bash
# Build the Linux launcher separately from the native Windows artifacts.
set -euo pipefail
project=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$project"
if [[ -x "$project/runtime/wsl-toolchain/cargo/bin/cargo" ]]; then
    export RUSTUP_HOME="$project/runtime/wsl-toolchain/rustup"
    export CARGO_HOME="$project/runtime/wsl-toolchain/cargo"
    export PATH="$CARGO_HOME/bin:/usr/local/bin:/usr/bin:/bin"
fi
export CARGO_TARGET_DIR="$project/target/wsl"
cargo build -p arcade-desktop
rustup target add i686-unknown-linux-gnu
mkdir -p runtime/wsl-libs
ln -sf /usr/lib/i386-linux-gnu/libpulse-simple.so.0 runtime/wsl-libs/libpulse-simple.so
ln -sf /usr/lib/i386-linux-gnu/libglut.so.3.12 runtime/wsl-libs/libglut.so.3
CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_RUSTFLAGS="-L native=$project/runtime/wsl-libs" \
    cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu --target-dir "$project/target"
