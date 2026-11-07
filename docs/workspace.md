# Workspace boundaries

- `crates/banana-content`: provider contracts, folder/ZAR/memory sources and archive conversion.
- `crates/banana-api`: compatibility/backend/session contracts and common process ownership.
- `crates/banana-core`: source-based executable inspection and reports.
- `crates/banana-vfs`: composition, persistent overlays, the view protocol host, FUSE/Android adapters and mount leases.
- `crates/banana-wire`: the dependency-free view protocol and its guest-side client.
- `guest/view-shim`: `banana_view.dll`, which serves native Windows titles' file calls from the view.
- `crates/banana-runtime`: game-agnostic orchestration, storage, dependency preparation and migration mechanisms.
- `game-compat/<title>`: everything one title needs. Layout:
  - `src/`: metadata, build guards, plan, transforms, settings profile, control layout and typed actions. Default `settings.cfg`/`controls.cfg` sections are generated from these declarations.
  - `guest/`, `hook/`, `io/`, `water/`: adapter crates (injected ABI implementations), picked up by the workspace globs. Plans reference them by artifact `name`.
  - `tests/`: the title's own contracts, such as defaults matching its adapter's built-in fallbacks.
  - `docs/`: startup notes, shipped as `docs/games/<title>/`.
  - `tools/`: development-only probes and capture scripts (not shipped).
- `backends/`: native, Wine and Water mechanisms. Water owns reusable ELF loading, graphics and audio translation.
- `guest/`: reusable guest services and platform input helpers.
- `crates/banana-compat`: composition root registering built-in modules.
- `apps/`: frontends; shared FFI remains in `crates/banana-ffi`.
- `tools/dist`: builds and packages crate-declared artifacts. The desktop app owns ZAR packing (`--pack` and its compressor window).

Game plan crates depend on shared contracts and content/input/settings services, rather than the session orchestrator. Backend crates implement the execution API. Title adapters may use a backend's published extension services.

Each shipped artifact is declared using `[[package.metadata.banana.artifacts]]` in its owning Cargo manifest, with `name`, `kind`, `target`, `hosts` and `destination`. Artifact names are unique. The distribution tool filters by host, rejects unsafe/duplicate names and destinations, builds the declared targets, and writes a development artifact catalog plus the portable package.

Adding a game means adding its `game-compat/<title>/` folder and registering it in `crates/banana-compat` (its manifest and `games()`). Adding a content format means registering a provider. Neither requires title branches in the frontend, filesystem, orchestrator or backend.

Android execution plumbing lives in `backends/android/`: its Rust backend owns processes and transports, while the PRoot extension only translates namespace operations into the shared Rust filesystem service. Kotlin owns the UI. Skylanders uses the same provider/plan contracts and crate-declared adapter artifacts as other titles.
