# Banana Pepper

Banana Pepper is a Rust compatibility framework for running PC-based arcade
cabinet software on ordinary desktops. Cabinet builds expect fixed hardware:
an I/O board, light guns or wheels, a dongle, a fixed display mode and a
custom boot environment. Banana Pepper replaces those pieces with desktop
equivalents, and it never modifies the original files. It does that with:

- **Inspection:** it identifies the executable format and fingerprints each build.
- **Runtime copies:** it prepares a verified copy for running, separate from the originals.
- **Sandboxes:** it launches the copy in an isolated sandbox.
- **Adapters:** it translates cabinet input, display and audio to the desktop.

It is a modular framework, not a set of individual launchers. A shared core
supplies discovery, settings, input and content handling. Each supported title
is a small crate that declares what it needs. Execution backends supply the
mechanisms, and frontends present the result.

No TeknoParrot code, profiles or loaders are used. This repository contains
only the framework and tools. Game executables, assets, cabinet records, saves
and third-party libraries are not included; you supply your own.

## Backends

| Backend | Runs | Host |
| --- | --- | --- |
| **Native** | Linux cabinet builds directly, with a 32-bit guest shim, inside Bubblewrap | Linux |
| **Wine** | Windows cabinet builds in a private Wine prefix, with a freestanding adapter DLL | Linux |
| **Water** | Linux cabinet builds as Windows processes: guest x86 code runs directly while its Linux, OpenGL, input, C-library and audio interfaces are translated to Windows APIs | Windows |
| **WSL** | Linux builds through WSL2 / WSLg | Windows |

[**Water**](backends/water/README.md) (*Water Adapts The ELF Runtime*) is the
project's Linux-to-Windows layer. It is a compatibility boundary, not CPU
emulation and not a security sandbox. Each title selects the backends it
supports; where several are available, its settings let you choose. Running on
macOS, or on ARM CPUs on the desktop, is not supported.

The guest-side pieces are shared too:

- **Linux shim:** the 32-bit library (`guest/linux-shim`) turns cabinet fullscreen requests into ordinary windows. It also bridges OSS audio to PulseAudio/PipeWire and reads controllers.
- **Windows runtime:** the freestanding Windows guest runtime (`guest/win32`) provides XInput and DirectInput readers to the adapter DLLs and Water.

An Android frontend (`apps/android`) drives the same framework through a UniFFI
API (`crates/banana-ffi`). It runs x86 guests with box64/box32 inside a proot rootfs.

## Build and run

Requires a current Rust toolchain and desktop graphics development libraries.
On Debian/Ubuntu the egui/glow build may need `build-essential pkg-config
libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev
libgl1-mesa-dev libwayland-dev libxkbcommon-dev`. Building the ZArchive reader
also needs a C++20 compiler.

```sh
cargo run -p banana-desktop
```

With `just` installed, `just` builds the release app and the guest components
for your host and assembles a portable folder under
`target/dist/banana-pepper-<os>-x86_64/`; `just run` builds and opens the workbench.
Copy the entire distribution folder to a writable location to use or share it.
It contains the workbench, ZAR converter, compiled helpers, adapters,
documentation, fresh default settings and a SHA-256 file manifest. It excludes
your game files, saves, caches and personal settings. Build output is replaced
on each build, so use a separate copy for play. WSL guest components and external
runtime libraries remain separate prerequisites, described in its `README.txt`.
All registered game backends are Rust; distributions contain no runtime scripts
and require no Python or shell interpreter. `just build-wsl` on Windows builds
and packages the optional Rust Linux hosts and guest adapters in WSL.
The package finds resources beside its executable and needs no source checkout.
`arcade-desktop --runtime-info` shows the resolved settings and cache paths.
Windows builds require
LLVM's `lld-link` for the 32-bit guest components. Linux builds additionally
prepare the local runtime libraries and require a 32-bit C linker/development
libraries plus Xvfb. Linux guest components for WSL must be built inside Linux;
see [Big Buck on Windows](docs/big-buck-windows.md) and
[Minecraft on Windows](docs/minecraft-windows.md) for setup.

Guest components are built for their own targets:

```sh
rustup target add i686-unknown-linux-gnu i686-pc-windows-msvc
cargo build -p banana-linux-shim --target i686-unknown-linux-gnu
```

The Windows adapter DLLs build with `--target i686-pc-windows-msvc`; see the
per-title notes in `docs/` for which a title needs.

Command line (`arcade-desktop`):

```sh
cargo run -p banana-desktop -- --scan [PATH]         # headless inspection report (JSON)
cargo run -p banana-desktop -- --games-root PATH     # open the workbench on a library
cargo run -p banana-desktop -- --bogus               # prints usage, including each title's --launch flag
```

Each title registers its own launch and diagnostic flags. Run the binary with
an unknown option to list them. Test the framework without desktop
dependencies with `cargo test -p banana-compat`.

## The workbench

- **Global library** lists every supported title, including experimental ones.
  **Add to My library** puts a title in **My library**, which is saved between
  launches. Nothing is added automatically. Removing a title keeps its files
  and settings.
- Each title's page shows its inspection report: format, architecture, imports,
  build fingerprint and compatibility notes. It also has **Settings** and a launch button.
  **Export JSON** writes the report to `arcade-report.json`.
- **Theme** in the header picks an accent colour. It is saved with your library
  preferences.

## Game sources and ZArchives

Set each title's folder or ZArchive in its **Settings → Game folder or
ZArchive**. Sources are saved per title. Titles without one use the `games`
folder next to the working directory.

ZArchives (`.zar`, read with [`zarchive-rs`](https://crates.io/crates/zarchive-rs))
can hold one title's folder or a whole library, with its layout preserved. On
first load an archive is checked for integrity and extracted into
`runtime/archive-cache/` (or `ARCADE_ARCHIVE_CACHE`). Later loads reuse the
cache by hash, and the archive itself is never modified. ELF files and shebang
scripts regain their executable bit on Linux. Every backend receives the same
extracted folder.

The **ZAR converter** (`tools/zar`) creates compatible archives:

```sh
cargo run -p banana-zar                                   # window: choose a folder and a destination
cargo run -p banana-zar -- --convert "games" "games.zar"  # no window
```

It compresses on a worker thread with progress and cancellation, and verifies
the archive before publishing it. Source files are kept, and existing outputs are
never replaced. Symbolic links are copied as ordinary entries; broken links are
skipped and reported. Link cycles, special files, non-UTF-8 names and names
differing only by case are rejected. The converter applies the loader's own
path, depth and size limits.

## Settings

Each title's **Settings** window has tabs for **Game files**, **Controls**,
**Graphics** and **Audio**. Choices are saved to `settings.cfg` in the project
root (one `[title]` section each), validated against what that title's
runtime supports, and applied the next time it starts.

- **Graphics:** resolution, windowed or fullscreen (on the monitor the window
  opens on), VSync (game default, on or off) and renderer (GPU, or Mesa's
  llvmpipe in software). Titles with light-gun aiming can also draw an
  on-screen crosshair: red or blue, normal or large, with an adjustable size.
- **Audio:** on or off, and an optional PulseAudio/PipeWire sink
  (`pactl list short sinks`). On Windows backends, sound goes to the default output.
- **Backend:** for titles with more than one backend on the host.
- **Players and input:** see below.

## Input

Bindings live in `controls.cfg` in the project root. Edit them in a title's
**Settings → Controls** tab, or edit the file directly. Changes apply the next
time the title starts. Escape always exits, and **Back + Start** together exit
from a controller.

### Players and input methods

The Controls tab has a menu per player: **Player 1**, plus **Player 2** for
two-player titles with **2 players** selected (`players = 2`). Each player
picks an **input method** (`p1_input` / `p2_input`):

| Method | Devices | Linux | Windows / Wine |
| --- | --- | --- | --- |
| **Keyboard & mouse** (`keyboard_mouse`) | The desktop keyboard and system pointer. One player at most | X11 / SDL / GLUT events | Window messages |
| **XInput** (`xinput`, default) | Xbox-style controllers | evdev gamepads with the Xbox layout | XInput |
| **DirectInput** (`dinput`) | Other controllers: generic pads, arcade sticks, wheels. Numbered buttons (1 = A ... 8 = RT, 10 = Start), POV hat as the d-pad | evdev joysticks | DirectInput 8 |
| **Raw Input** (`rawinput`) | A mouse or light gun of the player's own, so two can be told apart. Left = RT, right = LT, middle = A, side = Start, extra = Back. Absolute light guns point directly | evdev mice and absolute pointers | Raw Input (Water) |

Titles offer the methods that make sense for them (Raw Input needs aiming).
Devices of one kind go to the players using that method, in connection order.
If both players pick the same method, player 1 gets the first device and
player 2 the rest. The system mouse aims for the keyboard-and-mouse player, or
player 1 when nobody picked it. While anyone uses Raw Input, every mouse is
read directly and the system pointer only drives menus. On Linux the menu
lists the connected devices of the chosen kind.

Each player has its own controls. Player 2's menu has its own start and coin
keys, its own keys for player-specific actions (such as fire), and its own
controller buttons. Test, service and menu keys are shared with player 1.
Player 2's controller uses player 1's buttons until one is changed. It is
stored as a `[title.p2]` section, and **Reset player 2 to player 1's buttons**
goes back to sharing.

### Binding

Everything is bound by pressing it:

- **Keys:** click an action, then press the key (or a mouse button, where the
  title accepts one). **+** adds another key and **×** unbinds. A key already
  used elsewhere moves, and the other action takes the replaced key, so
  swapping two keys takes one click.
- **Controller, joystick, mouse and light-gun buttons:** click the action's
  **🎮** button, then press the button on a device of that player's method.
  **List** picks or removes buttons from a list. Clicking again or Esc
  cancels, as do 15 seconds without input.
- **Movement X and Y:** every method except keyboard & mouse has a
  **Movement** section: aim in titles with aiming, steering and gas/brake in
  titles with driving. Click a direction and move the stick, axis, pedal or
  light gun that way; moving it the opposite way binds it inverted. **+** adds
  a second axis (the one pushed further wins).

The desktop app reads devices the same way the titles will. On Linux that's
evdev for every method; on Windows it reads XInput live, and DirectInput or
Raw Input buttons are picked from **List**.

### File format

```ini
[title]
start1 = 1, PadStart          # keys first, then controller buttons
fire = MouseLeft, PadRT
move_x = AxisLX, AxisRX       # movement X: either stick
move_y = AxisLY               # movement Y; AxisLY- would invert it

[title.p2]                    # player 2's own controls (optional)
fire = K, PadA
```

- **Controller buttons:** `PadA`, `PadB`, `PadX`, `PadY`, `PadLB`, `PadRB`,
  `PadLT`, `PadRT`, `PadBack`, `PadStart`, `PadLS`, `PadRS` and `PadUp`/`Down`/`Left`/`Right`
  (Xbox names; DirectInput and Raw Input devices map onto them as above).
- **Axes:** `AxisLX`, `AxisLY`, `AxisRX`, `AxisRY`, `AxisLT`, `AxisRT` (for
  DirectInput: X, Y, Z, Rz, brake and gas) and `PointerX`, `PointerY` (Raw Input).
  A trailing `-` inverts an axis.
- **Keyboard names:** the workbench's spellings (`Enter`, `Space`, `Up`, `F1`, `Keypad5`, ...).

### Device access

- **Linux:** your user needs read access to `/dev/input/event*` devices
  (`uaccess` or the `input` group). Each sandbox is given only the device
  nodes its players' methods need. Keyboards and touchpads are never exposed,
  so keystrokes stay unreadable to the guest. Connect devices before a title
  starts.
- **Windows and Wine:** XInput (`xinput1_4`, `xinput1_3` or `xinput9_1_0`) and
  the system's DirectInput 8 are loaded the first time a player needs them.
  Under Wine, Wine's own XInput and DirectInput expose your Linux devices.

## Adding a title

A title is one crate in `game-compat/`. It declares:

- its executable fingerprints and how a runtime copy is prepared;
- its controls (actions, defaults, player-two mapping, movement axes);
- its settings profile (resolutions, renderers, crosshair, players, input methods);
- its supported backends and launch commands.

It is registered once in `crates/banana-compat`, the only place that lists
titles. The framework crates never name an individual title. See
[docs/workspace.md](docs/workspace.md) for the rules, and
[docs/architecture.md](docs/architecture.md) for platform boundaries.

## Layout

- `crates/`: the shared framework.
  - `banana-api`: game and backend traits, and the registry.
  - `banana-core`: discovery, executable identification and reports.
  - `banana-compat`: registration of built-in titles and backends.
  - `banana-runtime`: launch dispatch, plus settings and device hand-off.
  - `banana-settings`: `settings.cfg`.
  - `banana-input`: `controls.cfg`, the controller model and the evdev reader.
  - `banana-content`: folder and ZArchive sources.
  - `banana-ffi`: the Android API.
- `apps/desktop`: the egui/eframe workbench (`arcade-desktop`). `apps/android`: the Android app.
- `backends/`: `native`, `wine`, `water` and `wsl`.
- `guest/`: the i686 Linux shim, and the freestanding Windows guest runtime and input readers.
- `game-compat/`: one crate per title, with any title-specific guest DLLs beside it.
- `tools/zar`: the ZArchive converter. `tools/dist`: portable distribution assembly.
- `scripts/`: development/build helpers and cabinet-record capture tools; no game launchers.
- `docs/`: architecture notes and per-title startup and verification notes.
- `runtime/` and `games/`: generated runtime copies, caches and saves, and your local game files. Neither is under source control.
