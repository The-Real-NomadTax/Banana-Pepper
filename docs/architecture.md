# Mounting and execution architecture

## Ownership

`banana-content` defines immutable content sources and file handles: metadata, enumeration, random-access reads and fingerprints. Folder, ZAR and memory providers share the contract. A provider registry accepts further formats without modifying games or backends. The archive reader stays on a dedicated owning thread; its last handle closes and joins that worker.

`banana-api` defines build identification, compatibility plans, backend capability checks, prepared sessions, structured actions and session events. `banana-core` inspects providers without requiring host file paths or mounted content. Archive report paths are display locations inside a source, rather than extracted files.

`banana-vfs` composes source content, memory-backed generated files, aliases, dependency subtrees and a persistent overlay. Nothing is mounted on the host. Each backend exposes the view to its guest one of three ways:

- **Served (Windows).** The session host serves the view over private inherited pipes using the `banana-wire` protocol (`banana_vfs::wire::serve` on the host, the dependency-free `banana_wire::Client` in the guest). Water's workers implement the guest's file calls on it. Native Windows titles get a staged root holding only the view's directory tree and PE images; the staged executable imports `banana_view.dll` first, which hooks the Win32 file API (files, searches, mappings, INI profiles, share modes) so every other path under that root is served from the view. Nothing needs to be installed and no game content is extracted.
- **Namespaced (Linux).** `banana-sandbox` (built from `backends/native`) creates user and mount namespaces as an unprivileged process, opens `/dev/fuse` there (the kernel requires the device to come from the mounting namespace), mounts it and passes the connection back to the host over a socket; the host serves the view on it. In `run` mode it then builds an isolated root (read-only system directories, fresh `/dev`, `/proc` and `/tmp`, the desktop/GPU/audio binds) and runs native guests as PID 2 under a minimal init. In `hold` mode it keeps the host root and holds the namespace so Wine, its server and the game can enter it. The mount exists only inside the session and disappears with it: no fusermount, no Bubblewrap and no stale mounts. On Ubuntu 23.10 and later, AppArmor restricts unprivileged user namespaces; allow them for `helpers/banana-sandbox` with an AppArmor profile containing `userns,` (or system-wide).
- **Intercepted (Android).** The Android backend serves the view to `banana-tracer` (`SessionView::serve_traced`), which brokers the guest's filesystem calls into it.

`BANANA_VIEW_TRACE=1` (paths), `2` (every read, seek and size query) or `3` (also what is left to Windows) makes the Windows shim log to the session's `game.log`. With `3` and no view, the shim only observes, which shows how an unmodified game reads its files.

`banana-runtime` owns session orchestration and per-user storage. It selects a backend through registered capability checks, resolves declared artifacts and dependencies, creates the view, hands it to the backend, supervises execution, and releases it after the process tree ends. It contains no title branches.

Compatibility crates identify recognized builds and return plans. Their guest libraries own cabinet protocols, offsets, transforms, input mappings and diagnostics. `banana-compat` registers built-in games and backends. Frontends use these interfaces.

## Overlay and identity

Sources are never written. Overlay files store only dirty 64 KiB blocks plus an atomically replaced metadata index. Renames and deletion markers persist. Untouched assets and executable images are not materialized. Compatibility-generated executables remain in memory.

File descriptions retain identity across renames and remain usable after unlink. Replaced or deleted files with open descriptions stay accessible until those descriptions close. Unreferenced dirty blocks are reclaimed when the view closes. Metadata changes persist independently of file data.

PE views use ASCII-insensitive lookup and preserve source entry spelling; ambiguous names are rejected. ELF views retain case-sensitive lookup. Paths are relative to the virtual root. Folder providers use capability-relative filesystem access so links cannot escape the selected source.

Entry records bind a stable ID to a content fingerprint. The fingerprint streams content without extracting it. A changed source cannot reuse its previous overlay automatically. Each writable view holds an exclusive entry lock.


## Execution

Windows uses Native for supported PE plans and Water for supported ELF plans. Linux uses Native (in the session sandbox) or Wine (in a held session namespace). A Wine plan's guest mounts are drive letters (`F:` → a view folder); Wine sessions run without the system D-Bus, so the host's removable disks never appear as drives. Water runs Linux programs on Windows the way Wine runs Windows programs on Linux: its own dynamic linker, builtin Linux libraries (C library, GLUT/OpenGL, OSS audio) and, for static ELF64, trapped system calls. A title may add an optional preload adapter (`AdapterV2`), the same patch code as its Linux preload. See `backends/water/README.md`.

Windows children start suspended, join a kill-on-close job, and resume after assignment. Linux children run in owned process groups; native guests are additionally isolated by `banana-sandbox`. Session handles expose events, cancellation, commands and exit status. Time-limited diagnostics report a timeout distinctly.

Keyboard transport uses bounded, sequenced frames with focus state, held sources and edges. Receivers release held input after transport loss. Session actions use bounded structured messages. Game modules validate action payloads.

## State and migration

Configuration, durable overlays/dependencies, logs/captured records, and disposable caches use separate per-user directories. Resources are resolved from the package marker or development artifact catalog. Installed applications never fall back to a developer checkout for missing resources.

Legacy imports are declared by each game. Captured state is imported before planning; saves are imported after the overlay opens. An import ledger makes retries idempotent. Conflicts retain both copies. Old staged images, extracted assets and toolchains are not imported or deleted.

Android uses the same view preparation and sessions behind its native Kotlin UI. A Rust ptrace tracer (`banana-tracer`) brokers filesystem operations into the view and provides transient memfd backing for mapped ranges, without an OS mount or extracted game files. Generic Rust supervision owns Box64/Wine, Xvfb and frame/audio transports. See `android.md` for current limits and validation. Further Water game capabilities remain separate work.

Big Buck World illustrates title-family reuse: its plan mounts the supplied response corpus through the same view, its guest reads those files through Linux/Water library calls, and it shares Pro's cabinet input code. Live memory protection remains in the title adapter; durable game writes remain in the VFS overlay.
