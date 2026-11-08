# Workspace boundaries

- `crates/banana-content`: provider contracts, folder/ZAR/memory sources and archive conversion.
- `crates/banana-api`: compatibility/backend/session contracts and common process ownership.
- `crates/banana-core`: source-based executable inspection and reports.
- `crates/banana-vfs`: composition, persistent overlays, the view protocol host, the FUSE connection server (Linux) and the tracer view server (Android).
- `crates/banana-wire`: the dependency-free view protocol and its guest-side client.
- `guest/view-shim`: `banana_view.dll`, which serves native Windows titles' file calls from the view.
- `crates/banana-runtime`: game-agnostic orchestration, storage, dependency preparation and migration mechanisms.
- `game-compat/<title>`: everything one title needs. Layout:
  - `src/`: metadata, build guards, plan, transforms, settings profile, control layout and typed actions. Default `settings.cfg`/`controls.cfg` sections are generated from these declarations.
  - `guest/`, `hook/`, `io/`: adapter crates (the title's in-process patches), picked up by the workspace globs. Plans reference them by artifact `name`.
    An adapter is written once, for the platform the game was built for. A Windows title's hook runs natively on Windows and unchanged inside Wine elsewhere. A Linux title's guest library is preloaded on Linux and Android; on Windows the same crate is also built as a Water adapter (one artifact name, declared per host), so only the small per-platform frontend (window/input events and controller reading) differs.
  - `tests/`: the title's own contracts, such as defaults matching its adapter's built-in fallbacks.
  - `docs/`: startup notes, shipped as `docs/games/<title>/`.
  - `tools/`: development-only probes and capture scripts (not shipped).
- `backends/`: native, Wine and Water mechanisms. Water owns reusable ELF loading, graphics and audio translation.
- `guest/`: reusable guest services and platform input helpers. Session controller settings (`ARCADE_PLAYERS`, `ARCADE_PAD`, ...) are read by `banana_input::session` on every platform.
- `crates/banana-compat`: composition root registering built-in modules.
- `apps/`: frontends; shared FFI remains in `crates/banana-ffi`.
- `tools/xtask`: `cargo xtask`, the repository's only build tooling. It builds and packages crate-declared artifacts, builds the Android runtime inputs (the Debian rootfs from packages, Box64, Wine, Kotlin and x86-64 libraries) and the APK, and rebuilds committed fixtures. The desktop app owns ZAR packing (`--pack` and its compressor window).

Game plan crates depend on shared contracts and content/input/settings services, rather than the session orchestrator. Backend crates implement the execution API. Title adapters may use a backend's published extension services.

Each shipped artifact is declared using `[[package.metadata.banana.artifacts]]` in its owning Cargo manifest, with `name`, `kind`, `target`, `hosts` and `destination`. Artifact names are unique per host; one crate may declare the same name for different hosts. The distribution tool filters by host, rejects unsafe/duplicate names and destinations, builds the declared targets, and writes a development artifact catalog plus the portable package.

Adding a game means adding its `game-compat/<title>/` folder and registering it in `crates/banana-compat` (its manifest and `games()`). Adding a content format means registering a provider. Neither requires title branches in the frontend, filesystem, orchestrator or backend.

Android execution plumbing lives in `backends/android/`: its Rust backend owns processes and transports, while `banana-tracer` (`backends/android/tracer`) only translates namespace operations into the shared Rust filesystem service. Kotlin owns the UI. Skylanders uses the same provider/plan contracts and crate-declared adapter artifacts as other titles.
