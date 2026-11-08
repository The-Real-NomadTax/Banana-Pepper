set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Compile every artifact declared for this host and assemble a portable distribution.
build:
    cargo xtask dist

# Build, then start the packaged desktop application.
run *ARGS:
    cargo xtask run {{ARGS}}

# Build every Android runtime input this host can build, then the APK.
android:
    cargo xtask android all

[linux]
setup-libraries:
    cargo run -p banana-runtime --bin banana-session --release --locked -- --setup-legacy32
