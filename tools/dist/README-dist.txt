Banana Pepper desktop distribution

Start arcade-desktop[.exe] and select your own game folder or ZArchive.
arcade-zar[.exe] converts folders to archives. Keep the complete package together.

Package layout:
  arcade-desktop[.exe]   Desktop frontend
  arcade-zar[.exe]       Archive converter
  helpers/              Generic compiled execution workers and setup helpers
  games/<id>/<abi>/     Declared title adapter DLL/SO files
  docs/                 Architecture, setup and compatibility notes
  settings.cfg          Fresh default settings
  controls.cfg          Fresh default bindings
  manifest.json         Package artifact inventory
  THIRD-PARTY-NOTICES.txt

Windows requires WinFsp 2.1 or later and suitable graphics drivers. Linux
requires FUSE/fusermount, desktop graphics and Bubblewrap; PE games use Wine.
Required game libraries are prepared separately in per-user dependency storage.
Missing mounting components produce an error. Archives are never extracted as
a fallback. Generic Water workers support only their declared capabilities.

Original game content remains read-only. Patched executables are virtual files
backed by memory. All writes to the game view persist in a per-user dirty-block
overlay. No game trees, executables or untouched assets are copied for launch.

Configuration uses Windows APPDATA or Linux XDG_CONFIG_HOME. Durable data,
logs/captured records and caches use separate LOCALAPPDATA or XDG directories.
The installation can be read-only. --runtime-info prints the exact locations.
Legacy state imports preserve old files and report conflicts.

Generic CLI:
  arcade-desktop --scan SOURCE
  arcade-desktop --action GAME ACTION SOURCE [JSON]
  arcade-desktop --entries
  arcade-desktop --session-action ENTRY ACTION JSON

This package contains compiled Rust components and no runtime scripts, games,
cabinet records, saved preferences, captured seeds or cached game libraries.
No compiler or source checkout is required on the recipient's machine.

just build regenerates target/dist. Persistent user data is stored elsewhere.

Use `arcade-desktop --reset-overlay ENTRY [PATH]` to explicitly reset a file/subtree or, when PATH is omitted, all writes for that entry. Active sessions must close first. Original content and captured records remain intact.
