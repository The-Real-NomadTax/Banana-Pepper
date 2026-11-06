# Banana pepper

**[Water](backends/water/README.md) — Water Adapts The ELF Runtime** is our
Linux-to-Windows compatibility layer: Linux executables, Windows APIs.

Ford Racing Full Blown now has a native Windows runner and a shared Windows/Wine
cabinet adapter. Big Buck Hunter Pro also has a Water ELF32 worker:
its Linux game code runs directly in a 32-bit Windows process, with our own
OpenGL, input, C-library and audio translations. WSL2 remains selectable.
See [Ford setup](docs/ford-startup.md) and [Big Buck on Windows](docs/big-buck-windows.md)
for the verified results, commands and platform limits. These backends do not
establish macOS/Android or ARM CPU support.

A Rust arcade compatibility project, starting on Linux with an egui/eframe desktop workbench. No TeknoParrot code, profiles or loaders are used by this application.

The workbench also builds natively on Windows. Minecraft Dungeons Arcade has native Linux and Windows-to-WSL2 runners, library preparation, snapshot import repair and diagnostics. With dongle seeds captured locally and libraries prepared, **Play game** in the main UI uses the seeded runner with virtual joysticks, keyboard controls and card scanners. Coin, loadout, level selection and level startup have been reached; HUD issues and full gameplay verification remain. See [Linux setup and results](docs/minecraft-linux.md) or [Windows setup](docs/minecraft-windows.md).

**Current status:** Aliens Armageddon and Jurassic Park reach desktop gameplay with mouse controls. Big Buck Hunter Pro fully loads into scored gameplay with working score saving and verified PCM audio output through PulseAudio/PipeWire. Aliens and Jurassic audio now play through the desktop sound server (pending listening confirmation). Full playthroughs and physical cabinet hardware are unverified; Fast and Furious Super Cars (a Windows build) runs under Wine and reaches races with keyboard driving controls. See the per-game notes below for build requirements and limitations.

This repository contains the compatibility runtime and tools. Game executables, assets, cabinet records, saves, generated runtime files and third-party libraries are not included. Supply your own local game files under `games/`.

## Run

Requires a current Rust toolchain and desktop graphics development libraries. On Debian/Ubuntu the glow/X11/Wayland build may require `build-essential pkg-config libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev libgl1-mesa-dev libwayland-dev libxkbcommon-dev`.

From the project root:

```sh
cargo run -p banana-desktop
```

Configure each game’s folder or ZArchive in its **Settings → Game folder or ZArchive** field. Each game source is saved separately; unconfigured games use the default `games` folder relative to the working directory. Inspection runs on a worker thread. Export JSON saves `arcade-report.json` in the working directory.

Interactive Big Buck Hunter on your own display:

```sh
cargo run -p banana-desktop -- --launch-big-buck games
```

Or use **Play on my display** in the workbench. T = test, 1 = start, left mouse = fire, right mouse = pump, Esc = exit (keys are customizable, see below). An on-screen crosshair follows the mouse or controller aim. Choose it under the game's **Settings → Graphics → Crosshair**, or with `crosshair = off | p1 | p1_large | p2 | p2_large` in `settings.cfg`. The choices are red P1 or blue P2, each in a normal or large size, and the default is red P1. Set its size with the slider or `crosshair_size` (10–200 percent, default 50). Choose **Off** to remove it completely and keep the normal mouse pointer. The crosshair replaces the mouse pointer inside the game window. It works on Linux and in Water; the WSL backend does not draw it yet. Desktop mode has no automatic inputs or timeout. Its logs and save files live in `runtime/big-buck-desktop/`.

Headless inspection (no display required):

```sh
cargo run -p banana-desktop -- --scan games
```

To build/test only the portable core without desktop dependencies:

```sh
cargo test -p banana-compat
```

## ZArchived game folders

The workbench and existing desktop CLI commands accept ZArchives using
[`zarchive-rs`](https://crates.io/crates/zarchive-rs) 0.1.1. Archive the contents of
the folder you would normally select, preserving its layout. That can be a
supported individual game folder or the usual multi-game library folder.

Use the separate **Arcade ZAR Converter** app to create compatible archives:

```sh
cargo run -p banana-zar
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
cargo run -p banana-zar -- --convert "games/Big Buck Hunter Pro" "Big Buck Hunter Pro.zar"
cargo run -p banana-zar -- --convert "games" "games.zar"
```

Build a standalone executable with `cargo build --release -p banana-zar`. Run
`target/release/arcade-zar` on Linux or `target\release\arcade-zar.exe` on Windows.
It uses the same desktop build dependencies as the workbench. Each conversion
creates one archive; selecting a library packs all of its games into that archive.
Select the resulting `.zar` in the workbench, or pass it to the existing commands:

```sh
cargo run -p banana-desktop -- --scan "games.zar"
cargo run -p banana-desktop -- --launch-big-buck "Big Buck Hunter Pro.zar"
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

Xbox-style (XInput) and other (DirectInput) controllers work in every desktop game except Minecraft, and the gun games also take Raw Input mice and light guns (see Input methods below). Each action in `controls.cfg` can list controller buttons beside its keys, named `PadA`, `PadB`, `PadX`, `PadY`, `PadLB`, `PadRB`, `PadLT`, `PadRT`, `PadBack`, `PadStart`, `PadLS`, `PadRS`, `PadUp`, `PadDown`, `PadLeft` and `PadRight` (Xbox layout). For example, `start1 = 1, PadStart, PadA`. In the workbench, bind them the same way as keys: click an action's **🎮** button, then press the button on the device (**List** picks from a list, or removes buttons). **Back + Start** together exit any game.

| Game | Controller defaults | Built-in analog behaviour |
| --- | --- | --- |
| Super Cars, Ford | D-pad steers, RT = gas, LT = brake, Start/A = start, Back = coin, bumpers = gears/shift, face buttons = views | The left stick is an absolute wheel. Triggers bound to gas or brake are analog |
| Big Buck Hunter | RT/A = fire at the crosshair, LT/B = pump, Start, Back = coin | Either stick moves the crosshair |
| Jurassic Park | RT = fire, LT = secondary, Start, Back = coin, D-pad + A/B for menus | Either stick moves the aim |
| Aliens | Start/A = start, Back = coin, B = service/back, D-pad = volume/menu | Either stick moves the pointer. RT/LT click the left/right mouse buttons |

#### Input methods and player menus

Each game's **Settings → Controls** tab has a menu per player. Two-player games (Aliens, Jurassic Park, Big Buck Hunter) show **1 player / 2 players** and the **Player 1** and **Player 2** menus. The racers (Super Cars, Ford) show player 1's menu. In each menu, pick that player's **input method** (`p1_input` / `p2_input` in `settings.cfg`). Then set that player's own keys and buttons:

| Method | Reads | Linux (gun games) | Windows / Wine |
| --- | --- | --- | --- |
| **Keyboard & mouse** (`keyboard_mouse`) | The desktop keyboard and system mouse. Only one player can have it. The racers call it **Keyboard** | X11 / SDL / GLUT events | Window messages |
| **XInput** (`xinput`, the default) | Xbox-style controllers | evdev gamepads with the Xbox layout | XInput |
| **DirectInput** (`dinput`) | Other controllers: generic USB pads, arcade sticks, wheels. Buttons are numbered: button 1 = A ... button 8 = RT, button 10 = Start, POV hat = d-pad | evdev joysticks | DirectInput 8 (system `dinput8.dll`); controllers named Xbox are left to XInput |
| **Raw Input** (`rawinput`, gun games) | A mouse or light gun of the player's own, so two can be told apart. Left button = RT (fire), right = LT, middle = A, side = Start, extra = Back. Absolute light guns point directly; ordinary mice move the aim | evdev mice and absolute pointers | Raw Input (Water) |

Devices of one kind go to the players using that method in connection order. If both players pick it, player 1 gets the first device and player 2 the rest. The menu lists the connected devices of the chosen kind on Linux. The system mouse aims for the keyboard-and-mouse player, or player one when nobody picked it. While any player uses Raw Input, every mouse is read directly. In that case Raw Input players come first, any other mouse goes to the keyboard-and-mouse player, and the system pointer only drives menus. On Linux, each sandbox gets only the device nodes its methods need. Keyboards and touchpads are never exposed.

**Player 2's menu** has its own controls. Its Start and Coin rows are the Start 2 / Coin 2 keys plus player 2's controller buttons. Fire, secondary, trigger and pump get player 2's own keyboard keys and buttons. Test, service and menu keys are shared with player 1, but player 2's controller can have its own buttons for them. They're saved as a `[game.p2]` section in `controls.cfg` (`fire = K, PadA`). Until you change one of player 2's buttons, its controller uses player 1's layout, and **Reset player 2 to player 1's buttons** goes back to that. With `players = 1`, only player 1's menu applies.

**Press to bind.** Controller, joystick, mouse and light-gun buttons are bound like keys. Click the action's **🎮** button, then press the button on a device of that player's input method. That adds the button; clicking again or Esc cancels, and nothing pressed for 15 seconds cancels too. The app reads the devices the way the game will. On Linux that's evdev for every method. On Windows the app reads XInput controllers; pick DirectInput and Raw Input buttons from **List** there.

**Movement X and Y.** Every method except keyboard & mouse has a **Movement** section in the player's menu. In the gun games that's Aim X (right) and Aim Y (down); in the racers it's Steering (right) and Gas up / brake down. Click a direction, then move the stick, axis, pedal or light gun that way. Moving it the opposite way binds it inverted. **+** adds a second axis (the more deflected one wins), **×** unbinds and **List** picks from a list. They're saved as `move_x` / `move_y` in `controls.cfg`: `AxisLX`, `AxisLY`, `AxisRX`, `AxisRY`, `AxisLT`, `AxisRT` (for DirectInput: X, Y, Z, Rz, brake and gas axes) and `PointerX`, `PointerY` (Raw Input), with `-` for inverted. Defaults: either stick aims; the left stick steers, and Y is unbound in the racers. A racer's Y axis adds to the gas and brake bindings, so a combined pedal axis or a stick can drive both.

| Game | Player two |
| --- | --- |
| Big Buck Hunter | Second gun: its own crosshair in the other colour, and gun-two shots, trigger and pump. The mouse, its buttons and the keyboard trigger key move the keyboard-and-mouse player's gun. Linux and Water |
| Jurassic Park | Each player who aims without the system mouse aims through that player's gun inputs (controller stick, own mouse or light gun). The keyboard-and-mouse player aims with the mouse. The game draws each player's reticle |
| Aliens | The prepared image gives every player who aims without the system mouse its gun inputs. The mouse's player fires with the mouse buttons (RT/LT click them). Player 2 otherwise fires with **Q**/**W**. Player 1 fires with Q/W when player 2 has the mouse, or F13/F14 when nobody does (Raw Input) |

The input methods were built from the games' own input tables and the platform APIs, and are covered by unit tests. The Controls menus were checked on screen. No method has been played in a game yet, and the Windows DirectInput and Raw Input readers have only been compiled. DirectInput wheels steer with the X axis. A combined pedal axis can drive gas and brake through Movement Y; separate pedals that report as two axes still need gas and brake bound to wheel buttons.

How devices are read:

- **Windows and Wine** (Super Cars, Ford, and Big Buck through Water): XInput (`xinput1_4`, `xinput1_3` or `xinput9_1_0`) and DirectInput 8, loaded the first time a player needs them. Water also reads Raw Input mice. Under Wine, Wine's XInput and DirectInput expose your Linux controllers.
- **Linux** (Aliens, Jurassic, Big Buck): evdev. Xbox-style pads use the kernel's Xbox layout (wired `xpad` and Bluetooth `hid-microsoft` axis layouts are both handled), other joysticks the DirectInput numbering, and mice/light guns their buttons and X/Y. Your user needs read access to the device nodes, which you get from `uaccess` or the `input` group. **Connect devices before you start a game.** Aliens sends controller input as XTest events to the focused game window, which needs `libxcb-xtest` (32-bit).

Keyboard bindings for Aliens, Jurassic Park and Big Buck Hunter live in `controls.cfg` in the project root. Edit them in the workbench's **Controls** section (click an action, then press the new key; **+** adds an extra key, **×** unbinds, Esc cancels), or edit the file directly. Changes apply the next time a game starts. A key already used by another action moves to the new action, and the other action takes the replaced key, so swapping two keys takes one click. Escape is reserved for exiting.

| Game | Customizable | Fixed |
| --- | --- | --- |
| Aliens Armageddon | P1/P2 Start, Coin 1/2, **Test**, Service, Volume up/down (one key each) | Native mouse aim/fire |
| Jurassic Park 2015 | Starts, coins, Test, Service, menu keys, Fire, Secondary (any number of keys, including MouseLeft/MouseRight) | Mouse aim and menu pointer |
| Big Buck Hunter Pro | Starts, coins, Test, Service, keyboard trigger (ASCII keys only: no arrows/F-keys) | Mouse fire/pump |
| Fast and Furious Super Cars | Steering, gas, brake, start, views, music, gears, coins, Test, Service, volume (any number of keys) | Esc exits |

Aliens defaults: **T** opens the operator test menu, where T selects, **9** goes back and **Up/Down** move. Outside the menu Up/Down change volume, as on the cabinet.

## Android (Big Buck Hunter Pro)

An Android app in `apps/android/` runs Big Buck Hunter Pro on ARM64 phones. It uses box64 with box32 x86 translation inside a bundled Debian rootfs run by proot, with touch aiming, on-screen cabinet buttons and audio through AudioTrack. Users import their own game folder from the app. Gameplay was verified on a Galaxy S24 FE. Build with `apps/android/build-apk.sh`. [Android notes](docs/android.md) cover usage, architecture, the box64 and shim fixes, and the build.

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

See [docs/workspace.md](docs/workspace.md) for the full architecture and rules.

- `crates/`: the shared framework. `banana-api` (game and backend traits plus the registry), `banana-core` (discovery, executable identification, library reports), `banana-compat` (the only place that lists the built-in games), `banana-runtime` (launch dispatch, settings and controller hand-off), `banana-settings` (`settings.cfg`), `banana-input` (`controls.cfg`, controller model, crosshairs), `banana-content` (ZArchive extraction and caching), `banana-ffi` (UniFFI API for the Android app).
- `apps/desktop`: egui/eframe frontend (`arcade-desktop` binary). `apps/android`: the Android app.
- `game-compat/<game>`: one crate per title with its fingerprints, preparation, controls, settings and launchers. Guest DLLs/libraries for a single game live beside it (`super-cars/hook`, `ford-racing/hook`, `minecraft-dungeons/io`).
- `backends/`: reusable execution backends: `native`, `wine`, `water` (Linux-to-Windows, see [Water](backends/water/README.md)) and `wsl`.
- `guest/linux-shim`: 32-bit guest library adapting cabinet fullscreen requests into an ordinary GL window. `guest/win32`: shared freestanding Windows guest runtime and XInput sources.
- `tools/zar`: the folder-to-ZArchive converter.
- `runtime`: generated executable copies, save data, virtual-display tools and startup captures; excluded from source control.
- `docs/architecture.md`: platform boundaries and runtime implementation plan.
- `games`: local game files, excluded from source control.

Inspection preserves the source folder or archive; inspecting an archive writes its extraction cache. The separate Big Buck Hunter startup test prepares a hash-verified runtime copy and launches it inside Bubblewrap on a private Xvfb display. Original game files are preserved. Cabinet I/O and CPU emulation are not implemented.

API references: [eframe 0.36.2](https://docs.rs/eframe/0.36.2/eframe/) and [goblin object parser](https://docs.rs/goblin/0.10.7/goblin/enum.Object.html).

## Jurassic Park investigation

Jurassic Park now opens on the desktop with player-one mouse aiming and firing. Start reaches mission selection, and gameplay shots have produced a score of 75. Build the i686 shim, then run `cargo run -p banana-desktop -- --launch-jurassic games` or choose **Open Jurassic Park on my display**. **1** starts, **mouse** aims, **left click** fires, **T** opens test mode, and **Esc** exits. The Rust desktop profile uses the supplied plaintext scripts and omits unavailable cabinet security initialization. Main audio plays through PulseAudio/PipeWire; cabinet/rumble audio and physical cabinet I/O remain unfinished. Player two can use a second controller (see Two players above). The RIO connection overlay is suppressed in desktop mode. [Startup and control notes](docs/jurassic-startup.md) document the exact build checks and remaining work.

## Aliens Armageddon desktop runtime

Aliens now reaches playable 3D gameplay on the desktop; the user confirmed working controls and visuals. Build the i686 shim, then run `cargo run -p banana-desktop -- --launch-aliens games` or choose **Open Aliens on my display**. **1** starts, the native desktop mouse maps handle aiming/firing, **5** inserts a coin, **T** opens the test menu (all customizable) and **Esc** or the window's close button quits. The current copied settings are Spanish: choose **MODO HISTORIA** for Story Mode. Audio plays through PulseAudio/PipeWire; a full playthrough is unverified. Player two can use a second controller (see Two players above), which has not been played yet. [Aliens startup notes](docs/aliens-startup.md) document the validated asset seeds, graphics adapters and runtime-only changes. Original files remain unchanged.

## Fast and Furious Super Cars (Wine)

Super Cars is a Windows build. It runs under system Wine 11 with WineD3D in a private prefix (`runtime/supercars/prefix`), with the game mapped to `C:\rawart`. The bundled MohkerzLoaderz/TeknoParrot files are not used. Build the input adapter once with `rustup target add i686-pc-windows-msvc` and `cargo build -p banana-super-cars-hook --target i686-pc-windows-msvc --release`. Then run `cargo run -p banana-desktop -- --launch-super-cars games` or choose **Open Super Cars on my display**. **Left/Right** steer, **Up** = gas, **Down** = brake, **5** = coin (4 per credit), **Enter** = start, **T** = test menu and **Esc** exits. Keys are customizable, and XInput controllers drive with an analog stick and triggers (see Controllers above). The game logs a missing dongle but races still run. Non-XInput wheels, force feedback and the cabinet LED/H2O boards are not implemented. [Super Cars runtime notes](docs/supercars-startup.md) cover the adapter, graphics choices and remaining gaps.
