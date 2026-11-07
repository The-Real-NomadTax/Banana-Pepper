# Banana pepper

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

**Current status:** Aliens Armageddon and Jurassic Park reach desktop gameplay with mouse controls. Big Buck Hunter Pro fully loads into scored gameplay with working score saving and verified PCM audio output through PulseAudio/PipeWire. Aliens and Jurassic audio now play through the desktop sound server (pending listening confirmation). Full playthroughs and physical cabinet hardware are unverified; Fast and Furious Super Cars (a Windows build) runs under Wine and reaches races with keyboard driving controls. See the per-game notes below for build requirements and limitations.

This repository contains the compatibility runtime and tools. Game executables, assets, cabinet records, saves, generated runtime files and third-party libraries are not included. Supply your own local game files under `games/`.

## Run

Requires a current Rust toolchain and desktop graphics development libraries. On Debian/Ubuntu the glow/X11/Wayland build may require `build-essential pkg-config libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev libgl1-mesa-dev libwayland-dev libxkbcommon-dev`.

From the project root:

```sh
cargo run -p arcade-desktop
```

Configure each game’s folder or ZArchive in its **Settings → Game folder or ZArchive** field. Each game source is saved separately; unconfigured games use the default `games` folder relative to the working directory. Inspection runs on a worker thread. Export JSON saves `arcade-report.json` in the working directory.

Interactive Big Buck Hunter on your own display:

```sh
cargo run -p arcade-desktop -- --launch-big-buck games
```

Or use **Play on my display** in the workbench. T = test, 1 = start, left mouse = fire, right mouse = pump, Esc = exit (keys are customizable, see below). An on-screen crosshair follows the mouse or controller aim. Choose it under the game's **Settings → Graphics → Crosshair**, or with `crosshair = off | p1 | p1_large | p2 | p2_large` in `settings.cfg`. The choices are red P1 or blue P2, each in a normal or large size, and the default is red P1. Set its size with the slider or `crosshair_size` (10–200 percent, default 50). Choose **Off** to remove it completely and keep the normal mouse pointer. The crosshair replaces the mouse pointer inside the game window. It works on Linux and in Water; the WSL backend does not draw it yet. Desktop mode has no automatic inputs or timeout. Its logs and save files live in `runtime/big-buck-desktop/`.

Headless inspection (no display required):

```sh
cargo run -p arcade-desktop -- --scan games
```

To build/test only the portable core without desktop dependencies:

```sh
cargo test -p arcade-core
```

## ZArchived game folders

The workbench and existing desktop CLI commands accept ZArchives using
[`zarchive-rs`](https://crates.io/crates/zarchive-rs) 0.1.1. Archive the contents of
the folder you would normally select, preserving its layout. That can be a
supported individual game folder or the usual multi-game library folder.

Use the separate **Arcade ZAR Converter** app to create compatible archives:

```sh
cargo run -p arcade-zar
```

Choose a game folder (or the whole `games` library), choose a `.zar` destination
outside that folder, then click **Create .zar archive**. The app compresses on a
worker thread, displays progress, and supports cancellation. It includes the
folder's contents directly, preserving subfolders, empty directories and filenames.
Source files are kept, and existing output files are never replaced. Close games
and other programs that modify the source while converting. Symbolic links to files and directories are copied as ordinary archive entries,
including targets outside the selected folder. Broken symbolic links are skipped
and listed in the completion message. Directory link cycles, special files,
non-UTF-8 names and names differing only by ASCII case are rejected.
The converter applies the same path, depth and size limits as the loader and
verifies archive integrity before publishing the finished output. Cancellation
during finalization or verification waits for that operation to finish.

For conversion without a window:

```sh
cargo run -p arcade-zar -- --convert "games/Big Buck Hunter Pro" "Big Buck Hunter Pro.zar"
cargo run -p arcade-zar -- --convert "games" "games.zar"
```

Build a standalone executable with `cargo build --release -p arcade-zar`. Run
`target/release/arcade-zar` on Linux or `target\release\arcade-zar.exe` on Windows.
It uses the same desktop build dependencies as the workbench. Each conversion
creates one archive; selecting a library packs all of its games into that archive.
Select the resulting `.zar` in the workbench, or pass it to the existing commands:

```sh
cargo run -p arcade-desktop -- --scan "games.zar"
cargo run -p arcade-desktop -- --launch-big-buck "Big Buck Hunter Pro.zar"
```

Archives are checked for integrity and extracted into `runtime/archive-cache/`
on first load. Subsequent loads hash the archive and reuse its completed cache;
replacing its contents creates a new cache entry. The source archive is never
modified. Allow enough disk space for the uncompressed folder. Set
`ARCADE_ARCHIVE_CACHE` to use a different cache directory. You can remove unused
cache entries when no game is using them.

Source preparation happens before backend selection: Water and WSL on Windows,
and the existing Linux/Wine runners, receive the same extracted folder. Existing
game build checks and platform requirements still apply. ZArchive stores no Unix
permissions, so ELF files and shebang scripts regain executable permission on
Linux. Building the archive reader also requires a C++20 compiler; the crate
builds its bundled Zstandard implementation.

## Game libraries

**Global library** lists every supported game profile, including experimental games.
Select a game and choose **Add to My library**, then open it in **My library** and
set your local game folder or ZArchive in **Settings**. Game files are not included.
**My library** shows only games you have added; discovering files or refreshing
never adds games automatically. Your collection is saved between launches.
**Remove from My library** hides a game while preserving its files and settings.
Previously saved individual game folders are retained in your collection.

## Themes

Use the **Theme** selector in the top-right header to switch between **Classic teal**,
**Fiery red**, and **Midnight blue**. Accent colors apply immediately to the logo,
and controls. All themes keep Classic teal’s background colors. Your choice is saved with
your library preferences and restored at startup. Game graphics are configured separately.

## Game settings

Each playable profile (Aliens, Jurassic Park, Big Buck Hunter, Super Cars) has a **⚙ Settings** button on its page. The window has three tabs:

- **Controls**: key bindings, described below and saved to `controls.cfg`.
- **Graphics**: resolution, windowed or fullscreen, VSync (game default / on / off) and renderer (GPU or Mesa software).
- **Audio**: sound on/off and an optional PulseAudio/PipeWire output sink (`pactl list short sinks`).

Graphics and audio choices are saved to `settings.cfg` in the project root, one `[game]` section each, and apply the next time the game starts. The defaults reproduce the original launcher: Aliens and Jurassic at 1360x768 windowed on the GPU, Big Buck Hunter in a 640x480 window with software rendering. Resolution is passed to the game's own `-f` option (Aliens, Jurassic) or sets the window size (Big Buck Hunter, whose aiming is rescaled). Fullscreen fills only the monitor the game window opens on (Aliens asks the window manager for it, because its built-in SDL 1.2 fullscreen covers every monitor); choose that monitor's resolution for a full picture. Fullscreen and non-default sizes are experimental. Diagnostic and probe runs ignore these settings.


### Controllers

Xbox and other XInput-compatible controllers work in every desktop game except Minecraft. Each action in `controls.cfg` can list controller buttons beside its keys, named `PadA`, `PadB`, `PadX`, `PadY`, `PadLB`, `PadRB`, `PadLT`, `PadRT`, `PadBack`, `PadStart`, `PadLS`, `PadRS`, `PadUp`, `PadDown`, `PadLeft` and `PadRight` (Xbox layout). For example, `start1 = 1, PadStart, PadA`. In the workbench, use the **🎮** menu next to an action. **Back + Start** together exit any game.

| Game | Controller defaults | Built-in analog behaviour |
| --- | --- | --- |
| Super Cars, Ford | D-pad steers, RT = gas, LT = brake, Start/A = start, Back = coin, bumpers = gears/shift, face buttons = views | The left stick is an absolute wheel. Triggers bound to gas or brake are analog |
| Big Buck Hunter | RT/A = fire at the crosshair, LT/B = pump, Start, Back = coin | Either stick moves the crosshair |
| Jurassic Park | RT = fire, LT = secondary, Start, Back = coin, D-pad + A/B for menus | Either stick moves the aim |
| Aliens | Start/A = start, Back = coin, B = service/back, D-pad = volume/menu | Either stick moves the pointer. RT/LT click the left/right mouse buttons |

#### Two players

Aliens, Jurassic Park and Big Buck Hunter are two-player cabinets. Pick **2 players** at the top of the game's **Settings → Controls** tab, or set `players = 2` in that game's `settings.cfg` section. Controller 1 then plays player one and controller 2 plays player two, in the order the controllers were connected. Controller 2 uses player one's button layout: its Start, Coin, fire and secondary/pump act for player two, and its stick moves player two's aim. Test, service and menu buttons work from either controller. The mouse and keyboard stay player one. With the default `players = 1`, every controller plays player one, as before. The racing games are single-seat and Minecraft Dungeons has its own four-player setup, so they have no Players setting.

| Game | Player two |
| --- | --- |
| Big Buck Hunter | Second gun: its own crosshair in the other colour, and gun-two shots, trigger and pump. Linux and Water |
| Jurassic Park | Controller 2 drives the player-two gun inputs and its trigger/secondary. The game draws player two's reticle |
| Aliens | The two-player image maps player two's aim to controller 2. RT/LT press **Q**/**W**, the game's player-two fire keys |

Two-player mode was built from the games' own input tables and is covered by unit tests. It has not yet been played with two controllers in any game.

How controllers are read:

- **Windows and Wine** (Super Cars, Ford, and Big Buck through Water): XInput. The adapters load `xinput1_4`, `xinput1_3` or `xinput9_1_0` the first time they need it and merge up to four controllers (with two players, the first connected controller is player one and the others player two). Under Wine, Wine's own XInput exposes your Linux controllers.
- **Linux** (Aliens, Jurassic, Big Buck): evdev, with the kernel's Xbox layout. Wired `xpad` and Bluetooth `hid-microsoft` axis layouts are both handled. Each sandbox gets only controller device nodes. These are devices that report gamepad buttons and no letter keys, so keyboards are never exposed. Your user needs read access to them, which you get from `uaccess` or the `input` group. **Connect controllers before you start a game.** Aliens sends its controller input as XTest events to the focused game window, which needs `libxcb-xtest` (32-bit).

Keyboard bindings for Aliens, Jurassic Park and Big Buck Hunter live in `controls.cfg` in the project root. Edit them in the workbench's **Controls** section (click an action, then press the new key; **+** adds an extra key, **×** unbinds, Esc cancels), or edit the file directly. Changes apply the next time a game starts. A key already used by another action moves to the new action, and the other action takes the replaced key, so swapping two keys takes one click. Escape is reserved for exiting.

| Game | Customizable | Fixed |
| --- | --- | --- |
| Aliens Armageddon | P1/P2 Start, Coin 1/2, **Test**, Service, Volume up/down (one key each) | Native mouse aim/fire |
| Jurassic Park 2015 | Starts, coins, Test, Service, menu keys, Fire, Secondary (any number of keys, including MouseLeft/MouseRight) | Mouse aim and menu pointer |
| Big Buck Hunter Pro | Starts, coins, Test, Service, keyboard trigger (ASCII keys only: no arrows/F-keys) | Mouse fire/pump |
| Fast and Furious Super Cars | Steering, gas, brake, start, views, music, gears, coins, Test, Service, volume (any number of keys) | Esc exits |

Aliens defaults: **T** opens the operator test menu, where T selects, **9** goes back and **Up/Down** move. Outside the menu Up/Down change volume, as on the cabinet.

## Android (Big Buck Hunter Pro)

An Android app in `android/` runs Big Buck Hunter Pro on ARM64 phones. It uses box64 with box32 x86 translation inside a bundled Debian rootfs run by proot, with touch aiming, on-screen cabinet buttons and audio through AudioTrack. Users import their own game folder from the app. Gameplay was verified on a Galaxy S24 FE. Build with `android/build-apk.sh`. [Android notes](docs/android.md) cover usage, architecture, the box64 and shim fixes, and the build.

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
- `crates/arcade-content`: verified ZArchive extraction and shared game-source caching.
- `crates/arcade-desktop`: egui/eframe browser, Linux startup backend, report export and game settings.
- `crates/arcade-linux-shim`: 32-bit guest library adapting cabinet fullscreen requests into an ordinary GL window.
- `crates/arcade-wine-hook`: freestanding 32-bit Windows DLL, loaded by Wine as Super Cars' `dinput8.dll`, that feeds keyboard input to the game's cabinet I/O layer.
- `runtime`: generated executable copies, save data, virtual-display tools and startup captures; excluded from source control.
- `docs/architecture.md`: platform boundaries and runtime implementation plan.
- `games`: local game files, excluded from source control.

Inspection preserves the source folder or archive; inspecting an archive writes its extraction cache. The separate Big Buck Hunter startup test prepares a hash-verified runtime copy and launches it inside Bubblewrap on a private Xvfb display. Original game files are preserved. Cabinet I/O and CPU emulation are not implemented.

API references: [eframe 0.36.2](https://docs.rs/eframe/0.36.2/eframe/) and [goblin object parser](https://docs.rs/goblin/0.10.7/goblin/enum.Object.html).

## Jurassic Park investigation

Jurassic Park now opens on the desktop with player-one mouse aiming and firing. Start reaches mission selection, and gameplay shots have produced a score of 75. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-jurassic games` or choose **Open Jurassic Park on my display**. **1** starts, **mouse** aims, **left click** fires, **T** opens test mode, and **Esc** exits. The Rust desktop profile uses the supplied plaintext scripts and omits unavailable cabinet security initialization. Main audio plays through PulseAudio/PipeWire; cabinet/rumble audio and physical cabinet I/O remain unfinished. Player two can use a second controller (see Two players above). The RIO connection overlay is suppressed in desktop mode. [Startup and control notes](docs/jurassic-startup.md) document the exact build checks and remaining work.

## Aliens Armageddon desktop runtime

Aliens now reaches playable 3D gameplay on the desktop; the user confirmed working controls and visuals. Build the i686 shim, then run `cargo run -p arcade-desktop -- --launch-aliens games` or choose **Open Aliens on my display**. **1** starts, the native desktop mouse maps handle aiming/firing, **5** inserts a coin, **T** opens the test menu (all customizable) and **Esc** or the window's close button quits. The current copied settings are Spanish: choose **MODO HISTORIA** for Story Mode. Audio plays through PulseAudio/PipeWire; a full playthrough is unverified. Player two can use a second controller (see Two players above), which has not been played yet. [Aliens startup notes](docs/aliens-startup.md) document the validated asset seeds, graphics adapters and runtime-only changes. Original files remain unchanged.

## Fast and Furious Super Cars (Wine)

Super Cars is a Windows build. It runs under system Wine 11 with WineD3D in a private prefix (`runtime/supercars/prefix`), with the game mapped to `C:\rawart`. The bundled MohkerzLoaderz/TeknoParrot files are not used. Build the input adapter once with `rustup target add i686-pc-windows-msvc` and `cargo build -p arcade-wine-hook --target i686-pc-windows-msvc --release`. Then run `cargo run -p arcade-desktop -- --launch-super-cars games` or choose **Open Super Cars on my display**. **Left/Right** steer, **Up** = gas, **Down** = brake, **5** = coin (4 per credit), **Enter** = start, **T** = test menu and **Esc** exits. Keys are customizable, and XInput controllers drive with an analog stick and triggers (see Controllers above). The game logs a missing dongle but races still run. Non-XInput wheels, force feedback and the cabinet LED/H2O boards are not implemented. [Super Cars runtime notes](docs/supercars-startup.md) cover the adapter, graphics choices and remaining gaps.
