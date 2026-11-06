# Build the release app and both guest adapters from the repository root.
build:
    rustup target add i686-unknown-linux-gnu i686-pc-windows-msvc
    cargo build --workspace --release --locked
    # Existing launchers load the Linux shim from the debug directory.
    cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu --locked
    cargo build -p arcade-wine-hook --target i686-pc-windows-msvc --release --locked
    bash scripts/setup-desktop-runtime.sh

# Prepare runtime libraries and Xvfb without rebuilding the app.
setup-runtime:
    bash scripts/setup-desktop-runtime.sh

# Build everything, then open Arcade Foundry.
run: build
    ./target/release/arcade-desktop
