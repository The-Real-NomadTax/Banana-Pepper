# Water

Water Adapts The ELF Runtime runs Linux programs on Windows, the way Wine runs Windows programs on Linux. Guest code runs directly in a dedicated worker process. Water supplies everything the code would get from Linux: the dynamic linker, the libraries it uses and, where code makes them directly, system calls. Nothing is installed and no CPU emulation is involved.

## Structure

| Piece | Linux counterpart |
| --- | --- |
| `win32/linker.rs` | `ld.so`: maps the program and the ELF32 shared libraries the game ships, binds symbols (preload adapter, program, libraries in load order, then builtins), runs initializers, and provides `dlopen`/`dlsym`/`dlerror` |
| `win32/crt.rs`, `files.rs` | libc, libpthread, librt: start-up, files (served from the session view), time, threads, `setjmp` |
| `win32/display.rs` | GLUT and OpenGL on a Win32 window and WGL, with the program's own keyboard, mouse, timer, idle, reshape and display callbacks |
| `win32/sound.rs` | OSS audio on Windows audio |
| `windows.rs` | Static ELF64 programs with their system calls trapped (`water-x64`) |
| `extension.rs` | The preload adapter ABI (`AdapterV2`), Water's `LD_PRELOAD` |

Libraries Water implements itself are its *builtins* (`libc.so.6`, `libm.so.6`, `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libGL.so.1`, `libGLU.so.1`, `libglut.so.3`). Any other library a program needs is loaded from the game's own files if the game ships it. A function Water lacks is reported by name before the program runs, never skipped.

## Adapters are optional

A program runs with no adapter at all. A game patch is the same code as on Linux, built once more as a Water adapter DLL. It may replace any imported function, and it reaches Water's own implementation through `HostServicesV2::next`, like `dlsym(RTLD_NEXT)`. It can also patch the program's code after loading and observe window messages (for Raw Input). Big Buck's patch (`game-compat/big-buck-hunter/guest`) overrides the same three GLUT functions on Linux and in Water.

## Growing Water

Water grows the way Wine did: builtin library by builtin library, each one checked against real programs. The next steps, roughly in the order games need them:

1. **Thread-local storage.** i386 code reads TLS through `%gs`, which 32-bit Windows cannot point anywhere useful, so `gs:`-prefixed accesses have to be rewritten at load time. Shipped libraries using TLS relocations are rejected with a clear error until then.
2. **SDL 1.2 as a builtin** (window, OpenGL context, events, joystick, audio) on the same Win32 layer as GLUT. Jurassic Park needs this.
3. **X11 and XCB subsets** for programs that talk to the display directly (Aliens Armageddon's statically linked SDL).
4. **ELF64 dynamic programs**: the same linker for x86-64, with `%fs` TLS rewriting and glibc's start-up. Minecraft Dungeons needs this, plus libstdc++ and FMOD loaded as shipped libraries.

Build through `just build`. `cargo test -p banana-water` runs the ELF64 loader and execution checks, and on Windows `tests/elf32.rs`: a real 32-bit Linux program (built by `cargo xtask water-fixtures`) runs with a library it ships and a plugin it dlopens.
