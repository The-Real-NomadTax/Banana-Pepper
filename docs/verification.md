# Verification

Run `cargo test --workspace --locked` on Windows and on 64-bit Linux; both build without warnings. Then build and test the 32-bit targets: `--target i686-pc-windows-msvc` for Water and the Windows hooks and adapters (64-bit hooks such as Maximum Tune 6R's use `x86_64-pc-windows-msvc`; `cargo xtask dist` builds MSVC guest DLLs off Windows with lld-link and `panic=abort`), and `--target i686-unknown-linux-gnu` for the Linux game guests, which are empty crates on other targets.

Coverage checks provider parity and fingerprints; path containment; archive corruption and conversion; dirty-block persistence; metadata and case rules; stable identity and single writers; controls, controller frames and gun aiming modes; worker timeouts; static ELF64 execution; portable CLI archive inspection; and artifact metadata validation.

`cargo test -p banana-water --test elf32` (Windows) builds `water-x86` and runs a real 32-bit Linux program (`backends/water/tests/elf32`, rebuilt with `cargo xtask water-fixtures` on Linux) through Water's dynamic linker: it loads a library the game ships through `RUNPATH`, runs its constructor, copies its data into the program, and dlopens a plugin that binds back into it.

`cargo test -p banana-native --test namespace` (Linux) serves a real view into `banana-sandbox` in both modes: an isolated guest reads the view and a read-only sub-bind, writes into the overlay, runs as PID 2 with its own `/proc` and cannot write `/usr`; a held namespace serves processes that enter it, nothing is visible outside it, and the connection ends with it. Windows has no mount: Big Buck Hunter Pro (Water, from a ZAR) and Ford Racing (native, from a folder and a ZAR) run through the served view with no extra software installed.

Synthetic fixtures verify mechanisms. Gameplay, graphics, audio, save behavior and cabinet protocols must be checked separately for each real title/build/backend combination. Current mounted compatibility profiles remain experimental until those checks are completed. Water reports every capability it lacks (TLS, ELF64 dynamic linking, missing library functions) by name; see `backends/water/README.md`.

`just build` must create a host-specific package containing only declared compiled components, defaults, documentation and dependency notices. Validate it from a relocated, read-only installation with per-user state directories. No game content, extracted caches, local preferences, captured records or development tools belong in that package.


Android validation:

```text
cargo check -p banana-android -p banana-runtime --target aarch64-linux-android --locked
cargo build -p banana-ffi --release --target aarch64-linux-android --locked
```

The C parts of these builds need the NDK's compilers (`CC_aarch64_linux_android`, `CXX_aarch64_linux_android`, `AR_aarch64_linux_android`); `cargo xtask android apk` sets them.
`cargo test -p banana-android --test bridge` (Linux with Xvfb) checks the display
and audio bridges against a real X server, and `cargo test -p xtask` checks package
resolution, merged-/usr layout, trimming and APK assembly. `cargo xtask android all`
builds the complete APK (the rootfs, Box64, banana-tracer and the optional Wine inputs
described in `android.md`). Successful host tests do not establish phone gameplay
support.
