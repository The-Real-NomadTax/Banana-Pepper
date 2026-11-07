# AGENTS.md

Banana Pepper runs user-supplied PC arcade cabinet builds on Windows, Linux and Android. It is a Rust workspace (edition 2024) with an Android Kotlin UI. One pipeline:

**Folder/ZAR source → composed view (`banana-vfs`) → game plan (`game-compat/<game>`) → backend → session**

## Hard rules

- **Never write to sources and never extract.** Patched executables and configs are in-memory `generated` files in the view. Game writes go to the per-user overlay (dirty 64 KiB blocks).
- **Nothing gets installed on the host.** No WinFsp, no FUSE drivers or fusermount, no Bubblewrap, no helper apps. Windows serves the view over pipes (`banana-wire`). Linux mounts FUSE inside the session's own namespace (`banana-sandbox`). Android uses PRoot interception.
- **Everything title-specific lives in `game-compat/<game>/`:** offsets, hashes, transforms, controls, settings, adapters, docs and probes. Shared crates, backends, the orchestrator and frontends never name a game. The only exception is the registry in `crates/banana-compat`.
- **Write each patch once, for the game's own OS.**
  - **Windows games:** `hook/` DLLs run natively on Windows and unchanged under Wine on Linux and Android.
  - **Linux games:** the `guest/` crate is preloaded on Linux and Android, and the same crate is built as Water's optional preload adapter on Windows. Override the same library functions on both, and keep only device reading per platform (see `game-compat/big-buck-hunter/guest/src/{jamma,glut,linux,water}.rs`). Per-frame controller work goes through `banana_input::session::Players`.
- **Water grows like Wine:** add missing Linux functions or libraries to Water as generic builtins, never as title code (see `backends/water/README.md`).
- **Plans reference artifacts by `name`, never by path.** Declare each artifact in its crate's `Cargo.toml` under `[[package.metadata.banana.artifacts]]` (`name, kind, target, hosts, destination`). A name must equal the library/binary filename and be unique per host.
- **No interpreted launchers.** No Python or shell launch scripts. Dev-only probes go in `game-compat/<game>/tools/` or `scripts/`, and are never shipped.
- **Unsupported means an error.** Unsupported combinations fail with a clear session error, never a silent fallback.

## Map

| Path | Owns |
|---|---|
| `crates/banana-content` | Sources (folder, ZAR, memory), cheap `stamp`, streaming fingerprints |
| `crates/banana-vfs` | View composition, overlay, `wire::serve`, `serve_fuse` (Linux), PRoot server (Android) |
| `crates/banana-wire` | Dependency-free view protocol and guest client |
| `crates/banana-api` | `GameCompat`, `LaunchPlan`, `ExecutionBackend`, `SessionView`, process ownership |
| `crates/banana-runtime` | Orchestration, storage, fingerprint cache, dependencies, migration |
| `crates/banana-input` | controls.cfg, pad model, crosshairs, `session` (`ARCADE_*` env), Linux `evdev` |
| `crates/banana-compat` | The only place listing games and backends |
| `backends/native` | Windows PE (staging, PE import injection), Linux `banana-sandbox` |
| `backends/wine` | Wine prefix in a held session namespace |
| `backends/water` | Linux-on-Windows runtime, like Wine in reverse: `water-x86` (ELF32 dynamic linker plus builtin libc/GLUT/GL/OSS) and `water-x64` (static ELF64); optional preload adapters (`AdapterV2`) |
| `backends/android` | Box64 / Wine / PRoot plumbing |
| `guest/view-shim` | `banana_view.dll`: hooks Win32 file APIs to read the served view |
| `guest/linux-shim`, `guest/win32` | Reusable in-guest services, included by game crates via `#[path]` |
| `apps/desktop` | `arcade-desktop` (egui), including ZAR packing (`--pack`) |
| `apps/android` | Kotlin UI over `crates/banana-ffi` |
| `tools/dist` | Builds declared artifacts and writes `target/dist/…` plus `artifacts.json` |

Game crate layout: `src/` (metadata in `lib.rs`, `plan.rs`, `prepare.rs`, `settings.rs`, `controls.rs`, `android.rs`), `guest/` (Linux preload / Water adapter), `hook/` (Windows DLL), `io/`, `tests/defaults.rs`, `docs/`, `tools/`.

**Adding a game:** create `game-compat/<game>/` (the workspace globs pick it up). Implement `GameCompat` and a plan function returning `LaunchPlan`. Add the crate to `crates/banana-compat` (both `Cargo.toml` and `games()`).

## Commands

```bash
just build                                   # every artifact for this host -> target/dist/banana-pepper-<os>-<arch>/
just run                                     # build, then launch the packaged app
cargo test --workspace --locked              # the full suite (Windows and 64-bit Linux)
cargo build --release --locked -p <crate> --target i686-pc-windows-msvc    # hooks / Water adapters
cargo build --release --locked -p <crate> --target i686-unknown-linux-gnu  # Linux guest libraries (build on Linux/WSL)
cargo test -p banana-native --test namespace # Linux: real FUSE + namespace end-to-end
cargo test -p banana-water --test elf32      # Windows: a real ELF32 program through Water's linker
```

Real-game check: run `target/dist/<pkg>/arcade-desktop --action <game> probe "<source folder or .zar>"`. The session's `game.log` and captures land in the path it prints (`…/BananaPepper/state/entries/<id>/`). Other useful flags: `--runtime-info`, `--entries`, `--scan SRC`, `--reset-overlay ENTRY [PATH]`.

Debugging:
- `BANANA_VIEW_TRACE=1|2|3` traces the Windows shim's file traffic into `game.log`.
- `ARCADE_NATIVE_TRACE=1` traces Water's import calls.

## Gotchas

- Builds use `--locked`. After changing dependencies or crates, run a plain `cargo` command once so `Cargo.lock` updates, and commit the lock.
- i686 Windows uses `lld-link` with `panic=abort` and a static CRT (`.cargo/config.toml`). Hooks are freestanding and use `raw-dylib`, with no CRT or MSVC needed.
- Linux game guests are 32-bit: their crates are empty on other targets. Test them with `--target i686-unknown-linux-gnu`.
- 32-bit Windows code can't run tests under `cargo test` (`panic=abort`). Test it through a real worker, as `tests/elf32.rs` does.
- `#[path]` in a non-`mod.rs` file resolves relative to that file's directory. Shared `guest/win32/*.rs` expects sibling `pad`, `xinput` and `dinput` modules.
- Running `rustfmt` on a file also reformats `#[path]`-included shared files. Revert any changes to those files.
- PE views are case-insensitive and ELF views case-sensitive. A bare INI name such as `SYSTEM.INI` resolves to the Windows directory, not the game directory.
- `banana-sandbox` needs unprivileged user namespaces. On Ubuntu 23.10 and later that needs an AppArmor `userns,` profile.
- Never hold a lock while calling back into game code.
- Patch code must verify the build's SHA-256 and the expected prologue bytes before writing any hook.

## Working conventions

- Work on `main`, the only branch. Commit with focused messages, and push only when asked.
- Keep changes minimal and in the owning crate. Delete dead paths rather than leaving compatibility shims.
- Docs to keep in sync: `README.md`, `docs/{architecture,workspace,verification,android}.md`, `backends/water/README.md`, `game-compat/<game>/docs/`.
- Passing tests don't prove a game works. Gameplay, graphics, audio and saves must be checked per title, build and backend, so state what was and was not verified.
- Never commit or ship game content, captures, `runtime/`, `target/` or local settings.
