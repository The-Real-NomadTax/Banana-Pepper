# Water

**Water Adapts The ELF Runtime.**

Water is Arcade Foundry's Linux-to-Windows compatibility layer. It translates
Linux executable interfaces to Windows APIs while x86 guest instructions run
directly on the host CPU. The name expands recursively, like Wine's.

Development has resumed for Big Buck Hunter Pro. `water-buck` is the separate
32-bit Windows ELF32 worker for its measured imported APIs. See
[`docs/big-buck-windows.md`](../../docs/big-buck-windows.md) for build instructions,
verification and remaining limits. WSL2 stays available; Minecraft still uses it.

The current proof of execution loads constrained static x86-64 ELF programs in
a Windows worker process. Ordinary instructions execute directly; certified
Linux `syscall` sites are translated through a Windows exception handler.
Implemented calls cover basic file I/O, anonymous memory, clocks and exit.
Eight tests passed, including actual guest writes, mmap/mprotect/munmap, file
round trips, Linux error returns, BSS/initial stack, and parent timeout handling.

The original `water` ELF64 worker does not run dynamically linked games. ELF shared libraries, TLS,
Linux threads, signals, X11/GLX and graphics translation remain unimplemented.
Minecraft is not supported by the native backend. The worker is a compatibility
boundary, not a security sandbox for untrusted native code. No performance claim
has been established against WSL2.

```powershell
cargo test -p arcade-water
cargo run -p arcade-water --bin water -- inspect path\to\guest.elf
cargo run -p arcade-water --bin water -- run path\to\static.elf --root path\to\root --work path\to\work --timeout 10
```
