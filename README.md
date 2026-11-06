# Arcade Foundry

**[Water](crates/arcade-water/README.md) — Water Adapts The ELF Runtime** is our
Linux-to-Windows compatibility layer: Linux executables, Windows APIs.

Ford Racing Full Blown now has a native Windows runner and a shared Windows/Wine
cabinet adapter. Big Buck Hunter Pro also has a Water ELF32 worker:
its Linux game code runs directly in a 32-bit Windows process, with our own
OpenGL, input, C-library and audio translations. WSL2 remains selectable.
See [Ford setup](docs/ford-startup.md) and [Big Buck on Windows](docs/big-buck-windows.md)
for the verified results, commands and platform limits. These backends do not
establish macOS/Android or ARM CPU support.

A Rust arcade compatibility project, starting on Linux with an egui/eframe desktop workbench. No TeknoParrot code, profiles or loaders are used by this application.

The workbench also builds natively on Windows. Minecraft Dungeons Arcade has native Linux and Windows-to-WSL2 runners, library preparation, snapshot import repair and diagnostics. With dongle seeds captured locally, `scripts/minecraft-seeded.py` boots it to the Mojang Studios screen with four virtual joysticks; factory setup and card scanners still block attract mode and gameplay. See [Linux setup and results](docs/minecraft-linux.md) or [Windows setup](docs/minecraft-windows.md).

**Current status:** Aliens Armageddon and Jurassic Park reach desktop gameplay with mouse controls. Big Buck Hunter Pro reaches a rendered hunt scene and has verified PCM audio output through PulseAudio/PipeWire. Aliens and Jurassic audio now play through the desktop sound server (pending listening confirmation). Full playthroughs and physical cabinet hardware are unverified; Fast and Furious Super Cars (a Windows build) runs under Wine and reaches races with keyboard driving controls. See the per-game notes below for build requirements and limitations.

This repository contains the compatibility runtime and tools. Game executables, assets, cabinet records, saves, generated runtime files and third-party libraries are not included. Supply your own local game files under `games/`.

## Run

Requires a current Rust toolchain and desktop graphics development libraries. On Debian/Ubuntu the glow/X11/Wayland build may require `build-essential pkg-config libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev libgl1-mesa-dev libwayland-dev libxkbcommon-dev`.

From the project root:

```sh
cargo run -p arcade-desktop
```

The games folder defaults to `games` relative to the working directory and is editable in the UI. Inspection runs on a worker thread. Export JSON saves `arcade-report.json` in the working directory. Keyboard input monitoring is active while hovering over the aim area, with text entry unfocused.

Interactive Big Buck Hunter on your own display:

```sh
cargo run -p arcade-desktop -- --launch-big-buck games
```

Or use **Play on my display** in the workbench. T = test, 1 = start, left mouse = fire, right mouse = pump, Esc = exit (keys are customizable, see below). Desktop mode has no automatic inputs or timeout. Its logs and save files live in `runtime/big-buck-desktop/`.

Headless inspection (no display required):

```sh
cargo run -p arcade-desktop -- --scan games
```

To build/test only the portable core without desktop dependencies:

```sh
cargo test -p arcade-core
```

## Game settings

Each playable profile (Aliens, Jurassic Park, Big Buck Hunter, Super Cars) has a **⚙ Settings** button on its page. The window has three tabs:

- **Controls**: key bindings, described below and saved to `controls.cfg`.
- **Graphics**: resolution, windowed or fullscreen, VSync (game default / on / off) and renderer (GPU or Mesa software).
- **Audio**: sound on/off and an optional PulseAudio/PipeWire output sink (`pactl list short sinks`).

Graphics and audio choices are saved to `settings.cfg` in the project root, one `[game]` section each, and apply the next time the game starts. The defaults reproduce the original launcher: Aliens and Jurassic at 1360x768 windowed on the GPU, Big Buck Hunter in a 640x480 window with software rendering. Resolution is passed to the game's own `-f` option (Aliens, Jurassic) or sets the window size (Big Buck Hunter, whose aiming is rescaled). Fullscreen fills only the monitor the game window opens on (Aliens asks the window manager for it, because its built-in SDL 1.2 fullscreen covers every monitor); choose that monitor's resolution for a full picture. Fullscreen and non-default sizes are experimental. Diagnostic and probe runs ignore these settings.


Keyboard bindings for Aliens, Jurassic Park and Big Buck Hunter live in `controls.cfg` in the project root. Edit them in the workbench's **Controls** section (click an action, then press the new key; **+** adds an extra key, **×** unbinds, Esc cancels), or edit the file directly. Changes apply the next time a game starts. A key already used by another action moves to the new action, and the other action takes the replaced key, so swapping two keys takes one click. Escape is reserved for exiting.

| Game | Customizable | Fixed |
| --- | --- | --- |
| Aliens Armageddon | P1/P2 Start, Coin 1/2, **Test**, Service, Volume up/down (one key each) | Native mouse aim/fire |
| Jurassic Park 2015 | Starts, coins, Test, Service, menu keys, Fire, Secondary (any number of keys, including MouseLeft/MouseRight) | Mouse aim and menu pointer |
| Big Buck Hunter Pro | Starts, coins, Test, Service, keyboard trigger (ASCII keys only: no arrows/F-keys) | Mouse fire/pump |
| Fast and Furious Super Cars | Steering, gas, brake, start, views, music, gears, coins, Test, Service, volume (any number of keys) | Esc exits |

Aliens defaults: **T** opens the operator test menu, where T selects, **9** goes back and **Up/Down** move. Outside the menu Up/Down change volume, as on the cabinet.

## Included profiles

| Title | Inspected executable | Format observed |
| --- | --- | --- |
| Aliens Armageddon | `game` | ELF32, i386 |
| Jurassic Park 2015 | `game_original` | ELF32, i386 |
| Big Buck Hunter Pro | `game` in the supplied `Big Bug Hunter Pro` directory | ELF32, i386, no section table |
| Fast and Furious Super Cars | `sdaemon.exe` in the nested game directory | PE32, i386 |
| Minecraft Dungeons Arcade | supplied `gamedump.exec` (preferred) or `game` for inspection | ELF64, x86-64 |
| Ford Racing Full Blown | `FordRacing/fordracing.exe` | PE32, i386 |

Jurassic Park deliberately uses the supplied `game_original` rather than the active `game`, which the existing README describes as modified. This is a filename selection, not verification that the file is pristine. Super Cars selects the game executable rather than the bundled third-party loader. Original files and launch scripts are not modified or run. Imported library names are requirements recorded in the executable, not proof those libraries are installed or usable.

## Layout

- `crates/arcade-core`: static ELF/PE inspection, game reports, normalized cabinet input. No UI, graphics or process execution.
- `crates/arcade-desktop`: egui/eframe browser, Linux startup backend, report export and local input monitor.
- `crates/arcade-linux-shim`: 32-bit guest library adapting cabinet fullscreen requests into an ordinary GL window.
- `crates/arcade-wine-hook`: freestanding 32-bit Windows DLL, loaded by Wine as Super Cars' `dinput8.dll`, that feeds keyboard input to the game's cabinet I/O layer.
- `runtime`: generated executable copies, save data, virtual-display tools and startup captures; excluded from source control.
- `docs/architecture.md`: platform boundaries and runtime implementation plan.
- `games`: local game files, excluded from source control.

Inspection is read-only. The separate Big Buck Hunter startup test prepares a hash-verified runtime copy and launches it inside Bubblewrap on a private Xvfb display. Original game files are preserved. Cabinet I/O and CPU emulation are not implemented.

API references: [eframe 0.36.2](https://docs.rs/eframe/0.36.2/eframe/) and [goblin object parser](https://docs.rs/goblin/0.10.7/goblin/enum.Object.html).

## Jurassic Park investigation

Jurassic Park now opens on the desktop with player-one mouse aiming and firing. Start reaches mission selection, and gameplay shots have produced a score of 75. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-jurassic games` or choose **Open Jurassic Park on my display**. **1** starts, **mouse** aims, **left click** fires, **T** opens test mode, and **Esc** exits. The Rust desktop profile uses the supplied plaintext scripts and omits unavailable cabinet security initialization. Main audio plays through PulseAudio/PipeWire; cabinet/rumble audio, physical cabinet I/O and player-two aiming remain unfinished. The RIO connection overlay is suppressed in desktop mode. [Startup and control notes](docs/jurassic-startup.md) document the exact build checks and remaining work.

## Aliens Armageddon desktop runtime

Aliens now reaches playable 3D gameplay on the desktop; the user confirmed working controls and visuals. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-aliens games` or choose **Open Aliens on my display**. **1** starts, the native desktop mouse maps handle aiming/firing, **5** inserts a coin, **T** opens the test menu (all customizable) and **Esc** or the window's close button quits. The current copied settings are Spanish: choose **MODO HISTORIA** for Story Mode. Audio plays through PulseAudio/PipeWire; a full playthrough and player two are unverified. [Aliens startup notes](docs/aliens-startup.md) document the validated asset seeds, graphics adapters and runtime-only changes. Original files remain unchanged.

## Fast and Furious Super Cars (Wine)

Super Cars is a Windows build. It runs under system Wine 11 with WineD3D in a private prefix (`runtime/supercars/prefix`), with the game mapped to `C:\rawart`. The bundled MohkerzLoaderz/TeknoParrot files are not used. Build the input adapter once with `rustup target add i686-pc-windows-msvc` and `cargo build -p arcade-wine-hook --target i686-pc-windows-msvc --release`. Then run `cargo run -p arcade-desktop -- --launch-super-cars games` or choose **Open Super Cars on my display**. **Left/Right** steer, **Up** = gas, **Down** = brake, **5** = coin (4 per credit), **Enter** = start, **T** = test menu and **Esc** exits. Keys are customizable. The game logs a missing dongle but races still run. Analog wheels/gamepads, force feedback and the cabinet LED/H2O boards are not implemented. [Super Cars runtime notes](docs/supercars-startup.md) cover the adapter, graphics choices and remaining gaps.
