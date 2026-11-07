# Big Buck on Windows

The Windows plan uses generic `helpers/water-x86.exe` with the title-owned `games/big-buck/windows-x86/banana_buck_water.dll`. WinFsp mounts the selected folder or ZAR together with a persistent overlay and the reconstructed executable in memory.

Build with `just build`. Select the source in the desktop, or use:

```text
arcade-desktop --action big-buck launch SOURCE
arcade-desktop --action big-buck probe SOURCE
```

The recognized cabinet build is guarded by SHA-256 before reconstruction and patching. Cabinet addresses, switch/gun behavior and the measured diagnostic sequence belong to the compatibility adapter. Water provides the loader and generic Win32 graphics/audio services.

Saves and every other game write remain in the entry's per-user overlay. Diagnostic captures/logs use its state directory. Use `--runtime-info` and `--entries` to locate them. Legacy Big Buck save directories are imported when available; their original files remain intact.

Physical cabinet serial protocols remain unimplemented. The new mounting path requires gameplay validation on Windows; historical desktop results do not establish this refactor's support level.
