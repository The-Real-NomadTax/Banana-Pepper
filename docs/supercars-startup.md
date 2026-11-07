> Research record: paths and launch transcripts below describe the earlier development harness. Current launches use the provider/mount/session architecture in architecture.md. Generated images are memory-backed; durable state uses per-user directories. Revalidate gameplay through the new pipeline before treating historical results as current support.

# Fast and Furious Super Cars: Wine runtime

Super Cars is a Windows (PE32, i386, Direct3D 8) cabinet build, so unlike the Linux titles it runs under Wine. The bundled community loader (`MohkerzLoaderz.exe`, MinHook, `teknoparrot.ini`, `dxwnd.dxw`) is not used and is not mapped into the prefix.

Verified: attract mode, coin-up, start, stage select, a full race with keyboard steering/gas, the operator test menu, and Esc exit. Audio uses Wine's PulseAudio driver; it has not been checked by ear. Not verified: long sessions, the wheel calibration screens, and audits/high-score persistence across many sessions.

## Requirements

- Wine 11 with new-style WoW64 (tested: `wine-11.21`). System Wine works. Proton is not needed.
- LLVM `lld-link` and the Rust `i686-pc-windows-msvc` target, to build the input adapter:

```sh
rustup target add i686-pc-windows-msvc
cargo build -p banana-super-cars-hook --target i686-pc-windows-msvc --release
```

## Run

```sh
cargo run -p banana-desktop -- --launch-super-cars games
```

You can also use **Open Super Cars on my display** in the workbench. The Rust game crate checks `sdaemon.exe` against SHA-256 `0cddbdbf1d107d27dc668897d7851b6ac96b4349320551d800504b1c50b195ba`, then:

1. creates `runtime/supercars/prefix` on first launch and removes its `Z:` drive, so the game only sees `C:`;
2. maps the game into `C:\rawart`, the cabinet path. Large assets are symlinks to the originals. Small files the game rewrites (`prefs`, `audits`, `*.hst`, logs) are private copies made once and kept between sessions;
3. copies the adapter in as `C:\rawart\dinput8.dll` (`dinput8=n,b`);
4. for windowed play, sets a Wine virtual desktop of the chosen size. The game always requests an exclusive fullscreen device. For fullscreen, the virtual desktop is removed;
5. runs `sdaemon.exe -noerr -resN`. Logs go to `runtime/supercars/` (`game.log`, `arcade-hook.log`).

## Controls (`[super_cars]` in controls.cfg)

| Action | Default | Cabinet input code |
| --- | --- | --- |
| Steer left / right | Left / Right | analog `adc1` (ramped, recentres) |
| Gas / brake | Up / Down | analog `adc2` / `adc3` (on/off) |
| Start / select | Enter, 1 | `start1` 0x10000 |
| View 1-3 | C, V, B | 0x10001-0x10003 |
| Music | M | `tunes` 0x10004 |
| Gears 1-4 | Q, W, E, R | 0x10005-0x10008 |
| Coin 1 / 2 | 5 / 6 | 0x10009 / 0x1000a |
| Test / Service | T / 9 | 0x1000b / 0x1000c |
| Volume up / down | PageUp / PageDown | 0x1000e / 0x1000f |

Esc always exits. The operator menu is navigated with Volume +/- and View 1/View 3, selected with Test or Start, and left with Service or Music. Four coins make one credit with the supplied `prefs`.

## How the adapter works (`game-compat/super-cars/hook`)

The game reads cabinet controls through a Raw Thrills I/O library ("RTIO") that names each input (`start1`, `coin1`, `adc1`, …) and posts events to the sink at `0x4a9510(code, state)`. The adapter is a freestanding `no_std` Rust DLL (no CRT, `raw-dylib` imports, linked by `lld-link`). It:

- forwards `DirectInput8Create` to Wine's real dinput8;
- when it loads, checks the exact bytes at the per-frame input poll (`0x4a8e90`) and at the sink. It then redirects the poll through a trampoline in memory only. On any other build it logs and does nothing;
- checks and replaces the cabinet-availability query case at `0x4a9a6d` to report the adapter's controls as available. Car/track selection otherwise ignores the analog wheel and falls back to the suppressed developer keyboard. The real RTIO status and board polling are unchanged;
- after each poll, reads the game's raw DirectInput keyboard buffer (`0x4780108`). It posts press/release edges for button bindings and writes steering, gas and brake into the analog table at `0x477f808` (0..0xfff; the game maps `adc1` to -1..1);
- then clears the game's keyboard state so the developer shortcuts compiled into the game (Space, R, arrows, F-keys and others) never fire.

Bindings come from `ARCADE_SUPERCARS_KEYS` (`target:DIK,...`). Set `ARCADE_SUPERCARS_DEBUG=1` to log key transitions.

## Graphics

- **WineD3D (default):** Wine's built-in D3D8 over OpenGL, on the GPU through Wine's WoW64 GL path. The game draws its 2D menus at 1360x768 and depends on D3D8 Present stretching them to the backbuffer. WineD3D does this.
- **DXVK (opt-in, `ARCADE_SUPERCARS_DXVK=1`):** found in Steam's Proton installs, or `ARCADE_DXVK`. Races render correctly, but menus appear unscaled in the top-left corner. Not recommended.
- **Software:** WineD3D on Mesa llvmpipe.

Resolution sets the Wine window size and the game's `-res` option (640x480, 800x600, 1024x768, 1280x1024, 1360x768, 1600x1200, 1920x1080). The supplied `prefs` also store a 1920x1080 operator resolution.

## Known gaps

- `messages.txt` logs `DONGLE MISSING` periodically. Attract mode and races continue. No dongle emulation is implemented.
- H2O board, LED display (COM1) and the PortTalk parallel-port wheel/force-feedback driver are absent. The game logs each one once at startup and carries on.
- XInput controllers (Xbox and compatible) are read by the adapter: the left stick sets the steering position and triggers bound to gas/brake are analog (`ARCADE_PAD`, from controls.cfg). DirectInput-only wheels (the game's own `-joy`/`-g27` paths) are not wired in yet.
- The keypad (PIN entry, codes 0x10080-0x1008b) is not bound.
