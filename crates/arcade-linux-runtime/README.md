# Native Linux runtime (development paused)

Development is paused at the user's request while Minecraft work continues
through the WSL2 backend. WSL2 remains a supported Windows backend option and
is not replaced by this crate.

The current proof of execution loads constrained static x86-64 ELF programs in
a Windows worker process. Ordinary instructions execute directly; certified
Linux `syscall` sites are translated through a Windows exception handler.
Implemented calls cover basic file I/O, anonymous memory, clocks and exit.
Eight tests passed, including actual guest writes, mmap/mprotect/munmap, file
round trips, Linux error returns, BSS/initial stack, and parent timeout handling.

This crate does not run dynamically linked games. ELF shared libraries, TLS,
Linux threads, signals, X11/GLX and graphics translation remain unimplemented.
Minecraft is not supported by the native backend. The worker is a compatibility
boundary, not a security sandbox for untrusted native code. No performance claim
has been established against WSL2.

```powershell
cargo test -p arcade-linux-runtime
cargo run -p arcade-linux-runtime --bin linux-run -- inspect path\to\guest.elf
cargo run -p arcade-linux-runtime --bin linux-run -- run path\to\static.elf --root path\to\root --work path\to\work --timeout 10
```
