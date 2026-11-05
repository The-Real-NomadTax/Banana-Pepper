# Arcade Foundry

A Rust arcade compatibility project, starting on Linux with an egui/eframe desktop workbench. No TeknoParrot code, profiles or loaders are used by this application.

The workbench also builds natively on Windows. Minecraft Dungeons Arcade has native Linux and Windows-to-WSL2 runners, library preparation, snapshot import repair and diagnostics. Its Linux dependencies, including FMOD, now resolve locally on CachyOS as well as Ubuntu. Protected game assets and cabinet initialization remain unsupported, so Minecraft gameplay is **not working yet**. See [Linux setup and measured blocker](docs/minecraft-linux.md) or [Windows setup](docs/minecraft-windows.md).

**Current status:** Aliens Armageddon and Jurassic Park reach desktop gameplay with mouse controls. Big Buck Hunter Pro reaches a rendered hunt scene and has verified PCM audio output through PulseAudio/PipeWire. Aliens and Jurassic audio remain disabled. Full playthroughs and physical cabinet hardware are unverified; Fast and Furious Super Cars currently has executable inspection only. See the per-game notes below for build requirements and limitations.

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

Or use **Play on my display** in the workbench. T = test, 1 = start, left mouse = fire, right mouse = pump, Esc = exit. Desktop mode has no automatic inputs or timeout. Its logs and save files live in `runtime/big-buck-desktop/`.

Headless inspection (no display required):

```sh
cargo run -p arcade-desktop -- --scan games
```

To build/test only the portable core without desktop dependencies:

```sh
cargo test -p arcade-core
```

## Included profiles

| Title | Inspected executable | Format observed |
| --- | --- | --- |
| Aliens Armageddon | `game` | ELF32, i386 |
| Jurassic Park 2015 | `game_original` | ELF32, i386 |
| Big Buck Hunter Pro | `game` in the supplied `Big Bug Hunter Pro` directory | ELF32, i386, no section table |
| Fast and Furious Super Cars | `sdaemon.exe` in the nested game directory | PE32, i386 |
| Minecraft Dungeons Arcade | supplied `gamedump.exec` (preferred) or `game` for inspection | ELF64, x86-64 |

Jurassic Park deliberately uses the supplied `game_original` rather than the active `game`, which the existing README describes as modified. This is a filename selection, not verification that the file is pristine. Super Cars selects the game executable rather than the bundled third-party loader. Original files and launch scripts are not modified or run. Imported library names are requirements recorded in the executable, not proof those libraries are installed or usable.

## Layout

- `crates/arcade-core`: static ELF/PE inspection, game reports, normalized cabinet input. No UI, graphics or process execution.
- `crates/arcade-desktop`: egui/eframe browser, Linux startup backend, report export and local input monitor.
- `crates/arcade-linux-shim`: 32-bit guest library adapting cabinet fullscreen requests into an ordinary GL window.
- `runtime`: generated executable copies, save data, virtual-display tools and startup captures; excluded from source control.
- `docs/architecture.md`: platform boundaries and runtime implementation plan.
- `games`: local game files, excluded from source control.

Inspection is read-only. The separate Big Buck Hunter startup test prepares a hash-verified runtime copy and launches it inside Bubblewrap on a private Xvfb display. Original game files are preserved. Cabinet I/O and CPU emulation are not implemented.

API references: [eframe 0.36.2](https://docs.rs/eframe/0.36.2/eframe/) and [goblin object parser](https://docs.rs/goblin/0.10.7/goblin/enum.Object.html).

## Jurassic Park investigation

Jurassic Park now opens on the desktop with player-one mouse aiming and firing. Start reaches mission selection, and gameplay shots have produced a score of 75. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-jurassic games` or choose **Open Jurassic Park on my display**. **1** starts, **mouse** aims, **left click** fires, **T** opens test mode, and **Esc** exits. The Rust desktop profile uses the supplied plaintext scripts and omits unavailable cabinet security initialization. Audio, physical cabinet I/O and player-two aiming remain unfinished. The RIO connection overlay is suppressed in desktop mode. [Startup and control notes](docs/jurassic-startup.md) document the exact build checks and remaining work.

## Aliens Armageddon desktop runtime

Aliens now reaches playable 3D gameplay on the desktop; the user confirmed working controls and visuals. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-aliens games` or choose **Open Aliens on my display**. **1** starts, the native desktop mouse maps handle aiming/firing, and **5** inserts a coin. The current copied settings are Spanish: choose **MODO HISTORIA** for Story Mode. Audio remains disabled; a full playthrough and player two are unverified. [Aliens startup notes](docs/aliens-startup.md) document the validated asset seeds, graphics adapters and runtime-only changes. Original files remain unchanged.
