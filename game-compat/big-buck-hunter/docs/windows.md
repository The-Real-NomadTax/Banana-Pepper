# Big Buck on Windows

The Windows plan uses generic `helpers/water-x86.exe` with `games/big-buck/windows-x86/arcade_big_buck.dll`. That DLL is the same game patch as the Linux preload, built as Water's preload adapter: the virtual JAMMA board, switches, guns and controller aiming (`guest/src/jamma.rs`) and the GLUT window, callbacks and crosshairs (`guest/src/glut.rs`) are shared. Only controller reading differs (`guest/src/water.rs`: XInput, DirectInput and Raw Input). The session host serves the selected folder or ZAR to the worker over private pipes, together with a persistent overlay and the reconstructed executable in memory; nothing is mounted or installed.

Build with `just build`. Select the source in the desktop, or use:

```text
arcade-desktop --action big-buck launch SOURCE
arcade-desktop --action big-buck probe SOURCE
```

The recognized cabinet build is guarded by SHA-256 before reconstruction and patching. Cabinet addresses, switch/gun behavior and the measured diagnostic sequence belong to the compatibility adapter. Water provides the loader and generic Win32 graphics/audio services.

Saves and every other game write remain in the entry's per-user overlay. Diagnostic captures/logs use its state directory. Use `--runtime-info` and `--entries` to locate them. Legacy Big Buck save directories are imported when available; their original files remain intact.

Physical cabinet serial protocols remain unimplemented. The new mounting path requires gameplay validation on Windows; historical desktop results do not establish this refactor's support level.
