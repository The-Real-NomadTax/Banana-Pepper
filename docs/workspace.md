# Workspace boundaries

- `crates/banana-content`: provider contracts, folder/ZAR/memory sources and archive conversion.
- `crates/banana-api`: compatibility/backend/session contracts and common process ownership.
- `crates/banana-core`: source-based executable inspection and reports.
- `crates/banana-vfs`: composition, persistent overlays, WinFsp/FUSE adapters and mount leases.
- `crates/banana-runtime`: game-agnostic orchestration, storage, dependency preparation and migration mechanisms.
- `game-compat/<title>`: metadata, build guards, transforms, layout, dependencies, settings, controls and typed actions. Nested guest/adapter crates contain injected ABI implementations.
- `backends/`: native, Wine and Water mechanisms. Water owns reusable ELF loading, graphics and audio translation.
- `guest/`: reusable guest services and platform input helpers.
- `crates/banana-compat`: composition root registering built-in modules.
- `apps/`: frontends; shared FFI remains in `crates/banana-ffi`.
- `tools/dist`: builds and packages crate-declared artifacts.

Game plan crates depend on shared contracts and content/input/settings services, rather than the session orchestrator. Backend crates implement the execution API. Title adapters may use a backend's published extension services.

Each shipped artifact is declared using `[[package.metadata.banana.artifacts]]` in its owning Cargo manifest, with `name`, `kind`, `target`, `hosts` and `destination`. The distribution tool filters by host, rejects unsafe/duplicate destinations, builds the declared targets, and writes a development artifact catalog plus the portable package.

Adding a game means adding its compatibility crate and registering it. Adding a content format means registering a provider. Neither requires title branches in the frontend, filesystem, orchestrator or backend.
