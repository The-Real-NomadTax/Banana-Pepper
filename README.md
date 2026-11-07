# Banana Pepper

A Rust framework for running PC arcade cabinet builds on desktop systems and Android. Game executables, assets, cabinet records and third-party game libraries are supplied by the user.

Content flows through one shared pipeline:

**Folder/ZAR provider → composed virtual filesystem → compatibility plan → backend → session**

Original content stays read-only. Executable reconstruction and compatibility patches appear as virtual files backed by memory. Game writes persist in a per-user overlay containing changed blocks and filesystem metadata. Launching does not extract an archive or copy a game tree.

## Build and run

Install Rust, just, and the platform build tools, then run `just build`. The build reads artifact declarations from each crate and creates `target/dist/banana-pepper-<os>-<arch>/`. `just run` starts that package.

Windows needs nothing installed: Water and native Windows titles are served the game view directly over private pipes, with no OS mount (see [architecture](docs/architecture.md)). Linux needs `/dev/fuse`, unprivileged user namespaces and a desktop display; Windows games additionally require Wine. No fusermount or Bubblewrap: the package's own `banana-sandbox` helper mounts the served view inside each session's namespace. Missing components produce a session error. There is no extraction fallback.

| Host | Guest | Backend |
| --- | --- | --- |
| Windows x86-64 | Supported Windows PE builds | Native |
| Windows x86-64 | Supported Linux ELF builds | Water |
| Linux x86-64 | Supported Linux ELF builds | Native (session sandbox) |
| Linux x86-64 | Supported Windows PE builds | Wine |
| Android ARM64 (unrooted) | Experimental Linux / Windows plans | Box64 / Wine with the shared virtual filesystem |

Water runs Linux programs on Windows the way Wine runs Windows programs on Linux: its own dynamic linker plus builtin Linux libraries. It grows incrementally, and anything a program needs that Water lacks is reported by name (see [Water](backends/water/README.md)). The mounted architecture still requires gameplay validation for each title/build/backend combination.

## Storage

Windows configuration lives in `%APPDATA%/banana-pepper`; data, state and cache live under `%LOCALAPPDATA%/BananaPepper/`. Linux uses the XDG configuration, data, state and cache directories under `banana-pepper`.

Each library entry has a stable ID bound to its content fingerprint. Moving a source preserves that identity when the previous location is gone and the match is unique. Modified sources require overlay validation. One writable session may use an entry at a time.

The install directory can be read-only. Settings are initialized from packaged defaults or previous checkout settings. Compatibility modules declare which legacy saves or captured records can be imported. Imports preserve their sources and report conflicts. Existing generated game copies and development caches are left in place for manual housekeeping.

Use `arcade-desktop --runtime-info` to inspect storage paths, `--entries` to list entry IDs, `--scan SOURCE` to inspect a folder or archive without mounting it, and `--pack FOLDER OUTPUT.zar` to archive a game folder (also available from the desktop app's ZAR compressor).

## Architecture and actions

Game metadata, fingerprints, patches, layouts, cabinet protocols and action validation live under `game-compat/`. Backends provide execution mechanisms. Frontends and the session orchestrator use the registry and shared API.

Water workers are generic x86/x64 executables. Title adapters are DLL/SO libraries declared by their owning crates. The distribution contains compiled components, defaults, documentation and dependency notices.

`arcade-desktop --action GAME ACTION SOURCE [JSON]` starts a registered operation. `--session-action ENTRY ACTION JSON` sends a structured action to an existing session. Session actions are declared and validated by the compatibility module; Minecraft card scans use `scan-card` with `player` and `code` fields.

See [architecture](docs/architecture.md), [workspace boundaries](docs/workspace.md), [verification](docs/verification.md), and [Water](backends/water/README.md). Android uses a native Kotlin UI over the same Rust preparation and session API; its unprivileged namespace adapter remains experimental. See [Android](docs/android.md).

Use `arcade-desktop --reset-overlay ENTRY [PATH]` to explicitly reset a file/subtree or, when PATH is omitted, all writes for that entry. Active sessions must close first. Original content and captured records remain intact.
