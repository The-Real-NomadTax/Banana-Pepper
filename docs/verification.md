# Verification

Run `cargo test --workspace --locked` on Windows. Run the content, API, compatibility, runtime, filesystem and guest tests on Linux as well. Compile the i686 Windows Water worker and adapter targets, and the i686 Linux guest targets.

The real mount test requires an installed filesystem component and mount permission:

```text
cargo test -p banana-vfs --test mount -- --ignored --exact mounted_sources_execute_and_preserve_dirty_pages --nocapture
```

It runs a test executable directly from a memory-backed file in folder, ZAR and memory views. It exercises shared writable mappings, persistence, rename/delete with an open description, cleanup, and unchanged originals. Linux mounts happen inside each session's own namespace, so no fusermount-compatible host mountpoint is needed.

Other coverage checks provider parity and fingerprints; path containment; archive corruption and conversion; dirty-block persistence; metadata and case rules; stable identity and single writers; stale mount cleanup; controls and gun aiming modes; worker timeouts; static ELF64 execution; portable CLI archive inspection; and artifact metadata validation.

`cargo test -p banana-native --test namespace` (Linux) serves a real view into `banana-sandbox` in both modes: an isolated guest reads the view and a read-only sub-bind, writes into the overlay, runs as PID 2 with its own `/proc` and cannot write `/usr`; a held namespace serves processes that enter it, nothing is visible outside it, and the connection ends with it. Windows has no mount: Big Buck Hunter Pro (Water, from a ZAR) and Ford Racing (native, from a folder and a ZAR) run through the served view with no extra software installed.

Synthetic fixtures verify mechanisms. Gameplay, graphics, audio, save behavior and cabinet protocols must be checked separately for each real title/build/backend combination. Current mounted compatibility profiles remain experimental until those checks are completed. ELF64 dynamic linking, PIE and TLS remain explicit Water capability blockers.

`just build` must create a host-specific package containing only declared compiled components, defaults, documentation and dependency notices. Validate it from a relocated, read-only installation with per-user state directories. No game content, extracted caches, local preferences, captured records or development tools belong in that package.


Android/rootless validation:

```text
BANANA_TEST_PROOT=/absolute/path/to/patched/proot cargo test -p banana-vfs --test mount -- --ignored --exact rootless_namespace_executes_and_preserves_mapped_writes --nocapture
cargo build -p banana-ffi --release --target aarch64-linux-android --locked
```

Compile Kotlin sources against the Android SDK and compile the PRoot glue with the
NDK. A full APK needs the rootfs, Box64 and optional Wine inputs described in
`android.md`; successful host tests do not establish phone gameplay support.
