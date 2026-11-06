# Build the release app and both guest adapters from the repository root.
build:
    rustup target add i686-unknown-linux-gnu i686-pc-windows-msvc
    cargo build --workspace --release --locked
    # Minecraft and the existing Linux launchers load adapters from the debug directory.
    cargo build -p banana-minecraft-io --locked
    cargo build -p banana-linux-shim --target i686-unknown-linux-gnu --locked
    cargo build -p banana-super-cars-hook --target i686-pc-windows-msvc --release --locked
    bash scripts/setup-desktop-runtime.sh

# Prepare runtime libraries and Xvfb without rebuilding the app.
setup-runtime:
    bash scripts/setup-desktop-runtime.sh

# Build everything, then open Banana pepper.
run: build
    ./target/release/arcade-desktop
