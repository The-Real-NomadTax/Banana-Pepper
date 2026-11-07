# Windows builds use PowerShell and do not require a Unix shell/toolchain.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Build the release app and the Linux/Wine guest adapters.
[linux]
build:
    rustup target add i686-unknown-linux-gnu i686-pc-windows-msvc
    cargo build --workspace --release --locked
    # Minecraft and the existing Linux launchers load adapters from the debug directory.
    cargo build -p banana-minecraft-io --locked
    cargo build -p banana-linux-shim --target i686-unknown-linux-gnu --locked
    cargo build -p banana-super-cars-hook --target i686-pc-windows-msvc --release --locked
    cargo build -p banana-ford-hook --target i686-pc-windows-msvc --release --locked
    cargo build -p banana-skylanders-touch --target i686-pc-windows-msvc --release --locked
    cargo run -p banana-runtime --bin banana-session --release --locked -- --setup-legacy32 .
    cargo run -p banana-dist --release --locked

# Build the release app, Windows guest adapters and 32-bit Water worker.
[windows]
build:
    rustup target add i686-pc-windows-msvc
    cargo build --workspace --release --locked
    cargo build -p banana-super-cars-hook -p banana-ford-hook --target i686-pc-windows-msvc --release --locked
    cargo build -p banana-water --bin water-buck --target i686-pc-windows-msvc --release --locked
    cargo run -p banana-dist --release --locked

# Prepare Linux runtime libraries and Xvfb without rebuilding the app.
[linux]
setup-runtime:
    cargo run -p banana-runtime --bin banana-session --release --locked -- --setup-legacy32 .

# Native Windows backends need no Linux runtime setup.
[windows]
setup-runtime:
    Write-Output 'Native Windows backends need no runtime setup. For WSL backends, see docs/big-buck-windows.md and docs/minecraft-windows.md.'

# Build the optional compiled Linux/WSL hosts and guest adapters, then package.
[windows]
build-wsl:
    powershell.exe -NoLogo -NoProfile -File scripts/build-wsl.ps1
    just build

# Build everything, then open Banana pepper.
[linux]
run: build
    ./target/release/arcade-desktop

# Build everything, then open Banana pepper.
[windows]
run: build
    ./target/release/arcade-desktop.exe
