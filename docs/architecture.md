# Runtime architecture and platform plan

## Scope

The supplied games contain native PC executables. On a compatible x86 Linux host, the first approach is to investigate native execution with a compatible 32-bit userspace and implement the cabinet interfaces in our own code. A full CPU emulator would add a much larger task without resolving those interfaces by itself.

The current source implements static analysis, local input state, and an experimental Linux startup backend for one hash-verified Big Buck Hunter build. It reaches game/adventure selection and a rendered Whitetail hunt scene on a private Xvfb display, with a 32-bit Rust window shim and an experimental game-side JAMMA API adapter for switch edges and mouse gun callbacks. See `big-buck-startup.md`. The physical serial protocol and full gameplay validation remain outstanding.

## Boundaries

1. **Portable core (implemented):** game identity, static binary metadata, diagnostic reports and normalized input. Goblin reads program/dynamic headers, so missing ELF section tables do not inherently require executable repair.
2. **Host backend (initial Big Buck Hunter Linux probe implemented; other backends planned):** architecture/ABI capability checks, process supervision, isolated filesystem layout, audio/video integration and logs. Backend capabilities must be explicit; a frontend compiling on a platform does not establish game support.
3. **Cabinet adapter (planned, per title/build):** verified executable fingerprint, measured device protocol, coin/start/service state, light guns or wheel/pedals, outputs and calibration. Unknown builds must not receive version-specific patches.
4. **Frontend (initial desktop implementation, including private startup capture):** library, diagnostics and input setup. UI events will feed core state, never directly implement a device protocol. Android will need a platform entry point and touch/device input handling.

Keep a 64-bit UI and any future 32-bit Linux guest shim in separate processes. A 64-bit shared library cannot be loaded into these 32-bit games. The host/backend interface should carry versioned input snapshots and events; shared state needs explicit disconnect and release semantics before game integration.

## Observations from these files

- All three Linux `game` executables are ELF32 i386 with `/lib/ld-linux.so.2` as interpreter.
- Aliens imports legacy OpenGL/GLU/GLUT, ESD, a hardcoded `/usr/lib/libcsv.so`, and declares `/usr/lib/PhysX/v2.8.1/` as RPATH. Installed 64-bit equivalents cannot satisfy a 32-bit process.
- Big Buck Hunter's supplied directory is spelled `Big Bug Hunter Pro`; the profile preserves that spelling.
- Jurassic Park has multiple executable variants and a pre-existing TeknoParrot setup. Its documentation is historical evidence, not a dependency or verified explanation for our runtime.
- Super Cars includes `sdaemon.exe` and a separate bundled loader. The profile inspects and runs `sdaemon.exe` directly; the loader is not used.

## Implementation sequence

1. Establish executable fingerprints and an import/symbol/device-path inventory for each build. Compare static findings to controlled runtime traces. Never run the supplied cabinet boot scripts on the desktop: they include host configuration operations.
2. Start with one Linux title. Establish a reproducible 32-bit runtime and private cabinet filesystem view. Record the first failure with logs; distinguish graphics/library failure from hardware or security service failure. Validate original hardware/services where required rather than assuming they can be omitted.
3. Specify its observed cabinet transport and message semantics. Implement a Rust adapter with captured, reproducible protocol tests and a diagnostic device simulator. The current mouse monitor is not that simulator.
4. Connect normalized gun/coin/start state, add device selection, calibration and focus/disconnect release handling. Confirm boot, attract mode, credit, start, play and clean shutdown.
5. Repeat for the next Linux titles; share only interfaces shown to be compatible.
6. Add a Windows host backend and per-game Windows I/O adapter. On Linux, Super Cars now runs under Wine (see `supercars-startup.md`), with its I/O adapter as a freestanding Windows DLL (`crates/arcade-wine-hook`). A native Windows backend would reuse that DLL.

## Platform support targets

| Host | Frontend | Execution work |
| --- | --- | --- |
| Linux x86-64 | Initial target | 32-bit Linux ABI/userspace plus per-game adapters |
| Windows x86-64 | Future desktop target | Native PE backend; Linux games additionally need a Linux runtime/VM or translation |
| macOS (especially Apple Silicon) | Future desktop target | Linux/Windows ABI and x86 execution solution; graphics translation as needed |
| Android (ARM64) | Big Buck Hunter app (`android/`, see `android.md`) | box64/box32 in a proot Debian rootfs; Xvfb + llvmpipe; touch input and AudioTrack bridges |

Do not advertise game support on any platform until tested there. The current core avoids Unix-only APIs, but cross-compilation and device testing remain outstanding.
