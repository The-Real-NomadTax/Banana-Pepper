set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Compile every artifact declared for this host and assemble a portable distribution.
build:
    cargo run -p banana-dist --release --locked -- build

# Start the freshly packaged desktop application.
[windows]
run: build
    ./target/dist/banana-pepper-windows-x86_64/arcade-desktop.exe

[linux]
run: build
    ./target/dist/banana-pepper-linux-x86_64/arcade-desktop

[linux]
setup-libraries:
    cargo run -p banana-runtime --bin banana-session --release --locked -- --setup-legacy32
