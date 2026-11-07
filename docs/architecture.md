# Mounting and execution architecture

## Ownership

`banana-content` defines immutable content sources and file handles: metadata, enumeration, random-access reads and fingerprints. Folder, ZAR and memory providers share the contract. A provider registry accepts further formats without modifying games or backends. The archive reader stays on a dedicated owning thread; its last handle closes and joins that worker.

`banana-api` defines build identification, compatibility plans, backend capability checks, prepared sessions, structured actions and session events. `banana-core` inspects providers without requiring host file paths or mounted content. Archive report paths are display locations inside a source, rather than extracted files.

`banana-vfs` composes source content, memory-backed generated files, aliases, dependency subtrees and a persistent overlay. Each backend reaches that view one of three ways, declared by `ExecutionBackend::needs_mount`:

- **Served (Windows).** The session host serves the view over private inherited pipes using the `banana-wire` protocol (`banana_vfs::wire::serve` on the host, the dependency-free `banana_wire::Client` in the guest). Water's workers implement the guest's file calls on it. Native Windows titles get a staged root holding only the view's directory tree and PE images; the staged executable imports `banana_view.dll` first, which hooks the Win32 file API (files, searches, mappings, INI profiles, share modes) so every other path under that root is served from the view. Nothing needs to be installed and no game content is extracted.
- **Mounted (Linux).** FUSE exposes the view for Native/Bubblewrap and Wine.
- **Intercepted (Android).** The PRoot tracer brokers filesystem calls into the same view.

`BANANA_VIEW_TRACE=1` (paths), `2` (every read, seek and size query) or `3` (also what is left to Windows) makes the Windows shim log to the session's `game.log`. With `3` and no view, the shim only observes, which shows how an unmodified game reads its files.

`banana-runtime` owns session orchestration and per-user storage. It selects a backend through registered capability checks, resolves declared artifacts and dependencies, creates the view, keeps the mount alive, supervises execution, and releases it after the process tree ends. It contains no title branches.

Compatibility crates identify recognized builds and return plans. Their guest libraries own cabinet protocols, offsets, transforms, input mappings and diagnostics. `banana-compat` registers built-in games and backends. Frontends use these interfaces.

## Overlay and identity

Sources are never written. Overlay files store only dirty 64 KiB blocks plus an atomically replaced metadata index. Renames and deletion markers persist. Untouched assets and executable images are not materialized. Compatibility-generated executables remain in memory.

File descriptions retain identity across renames and remain usable after unlink. Replaced or deleted files with open descriptions stay accessible until those descriptions close. Unreferenced dirty blocks are reclaimed when the view closes. Metadata changes persist independently of file data.

PE views use ASCII-insensitive lookup and preserve source entry spelling; ambiguous names are rejected. ELF views retain case-sensitive lookup. Paths are relative to the virtual root. Folder providers use capability-relative filesystem access so links cannot escape the selected source.

Entry records bind a stable ID to a content fingerprint. The fingerprint streams content without extracting it. A changed source cannot reuse its previous overlay automatically. Each writable view holds an exclusive entry lock.

Mount leases own the driver host, ownership lock and empty mountpoint. Stale cleanup removes only empty mountpoints with an available ownership lock; it never walks or deletes mounted content.

## Execution

Windows uses Native for supported PE plans and Water for supported ELF plans. Linux uses Native/Bubblewrap or Wine. Water has generic x86 and x64 workers. Its versioned C adapter interface validates structure sizes and separates generic loading, graphics and audio from title behavior.

Windows children start suspended, join a kill-on-close job, and resume after assignment. Linux children run in owned process groups; native guest isolation additionally uses Bubblewrap. Session handles expose events, cancellation, commands and exit status. Time-limited diagnostics report a timeout distinctly.

Keyboard transport uses bounded, sequenced frames with focus state, held sources and edges. Receivers release held input after transport loss. Session actions use bounded structured messages. Game modules validate action payloads.

## State and migration

Configuration, durable overlays/dependencies, logs/captured records, and disposable caches use separate per-user directories. Resources are resolved from the package marker or development artifact catalog. Installed applications never fall back to a developer checkout for missing resources.

Legacy imports are declared by each game. Captured state is imported before planning; saves are imported after the overlay opens. An import ledger makes retries idempotent. Conflicts retain both copies. Old staged images, extracted assets and toolchains are not imported or deleted.

Android uses the same view preparation and sessions behind its native Kotlin UI. A versioned PRoot adapter brokers filesystem operations into Rust and provides transient memfd backing for mapped ranges, without an OS mount or extracted game files. Generic Rust supervision owns Box64/Wine, Xvfb and frame/audio transports. See `android.md` for current limits and validation. Further Water game capabilities remain separate work.
