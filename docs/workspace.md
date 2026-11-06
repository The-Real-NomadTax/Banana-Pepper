# Banana Pepper workspace architecture

Banana Pepper is a modular compatibility framework, not a set of individual
launchers. The shared framework supplies the infrastructure, each game crate
declares what its title needs, execution backends supply the mechanisms, and
frontends present the result.

```
crates/        shared framework (no individual games)
  banana-api       GameCompat + ExecutionBackend traits, registry, shared types
  banana-core      discovery, executable inspection/identification, reports
  banana-compat    registers every built-in game and backend (the one list)
  banana-runtime   launch dispatch, settings/controller hand-off, content
  banana-settings  settings.cfg: schema, validation, persistence
  banana-input     controls.cfg, controller (pad) model, crosshairs
  banana-content   folder and ZArchive sources, extraction cache
  banana-ffi       UniFFI API for the Android frontend
apps/
  desktop          egui frontend (binary: arcade-desktop)
  android          Android app
game-compat/
  aliens-armageddon  big-buck-hunter  jurassic-park
  super-cars (+hook)  ford-racing (+hook)  minecraft-dungeons (+io)
backends/
  native  wine  water  wsl
guest/
  linux-shim       i686 LD_PRELOAD shim for Linux cabinet builds
  win32            freestanding Windows guest runtime + XInput (shared source)
tools/
  zar              folder → ZArchive converter
```

## Dependency direction

```
banana-input ← banana-settings ← banana-api ← banana-core ← banana-runtime
                                     ↑              ↑             ↑
                               backends/*      game-compat/* ─────┘
                                     ↑              ↑
                                     └── banana-compat ── apps/desktop, banana-ffi
```

Game crates depend on the shared API; shared crates never depend on game
crates. `banana-compat` is the only shared crate that names games.

## Registration

Frontends call `banana_compat::install()` once at startup. It registers:

- every `GameCompat` and `ExecutionBackend` with `banana_api`,
- every game's `SettingsProfile` with `banana_settings`,
- every game's `ControlLayout` with `banana_input::controls`.

The settings and controls files, their validation and their UI are generated
from those declarations; no game writes its own settings implementation.

## Adding a game

1. Create `game-compat/<name>` depending on `banana-api` (and whichever of
   `banana-core`, `banana-runtime`, `banana-settings`, `banana-input` it
   uses).
2. Implement `GameCompat`: identity, executable location, known builds,
   status, notes, play hint, backends, optional settings profile, control
   layout, extra actions, CLI commands and `run`.
3. Add it to the workspace members/dependencies and to
   `banana_compat::games()`.

The core and frontends do not change. `tests/registry.rs` in
`banana-compat` checks ids, profiles and CLI flags for every registered game.

## Settings capabilities

A `SettingsProfile` declares the default resolution and renderer, crosshair
support, maximum players, selectable backends (shown on Windows when more
than one is listed) and whether audio is played by Windows. Frontends hide
options a profile does not declare.

## Rules

1. No game-specific code in shared crates.
2. One game compatibility API (`banana_api::GameCompat`).
3. One settings system, driven by declared capabilities.
4. One crate per game; multiple builds of a game live in that crate.
5. Backends implement mechanisms, never games.
6. Desktop and Android share the Rust backend.
7. Each frontend uses its platform's native UI.
8. Games are registered explicitly in `banana-compat`; nothing is loaded at
   runtime.
9. Binary patches apply only to identified builds, on staged copies.
10. Game crates depend on shared crates, never the reverse.
11. Registered game actions are implemented in Rust. Do not invoke Python or
    shell launchers, embed interpreter payloads, or ship runtime scripts.
    Game-specific preparation, input, saves and probe policy stay in the game's
    crate; reusable sandbox, Wine and WSL mechanisms belong in the backends.

## Compiled sessions

`banana-native::Sandbox` builds the isolated process filesystem and desktop,
GPU and audio interfaces. `banana-wine::Prefix` manages private Wine prefixes.
`banana-wsl` forwards argument vectors directly through `wsl.exe --exec` to
`banana-game-host`, which dispatches through the same `GameCompat` registry as
the desktop. `banana-session` supervises native guests and typed diagnostic
events, with owned child processes and bounded auxiliary tools. Neither helper
knows individual titles. Game crates own their images, mounts and event plans.

Minecraft's crate owns import reconstruction, captured-seed preparation,
library setup, direct evdev/uinput access and the compiled card utility.
The synthetic image regression test pins the old runner's byte-for-byte output.
No interpreter or source-checkout file is needed at runtime. Source-only build
and cabinet-record capture tools may still use platform scripting languages.

## Known gaps

This restructure is behaviour-preserving; these items still break the rules
above and are tracked for follow-up work:

- `guest/linux-shim` still contains Aliens, Jurassic Park and Big Buck
  modules. They should become per-game guest targets under `game-compat/`.
- `backends/water`'s `water-buck` worker carries Big Buck's JAMMA hooks. The
  game-specific parts should move to `game-compat/big-buck-hunter`, leaving
  Water as a generic ELF32 runtime.
- The desktop's settings window still has some per-tab layout that assumes
  the current set of settings; the backend selector, audio routing, status,
  hints and extra actions are now capability-driven.
- The Android app is still the Java Big Buck app. A Kotlin/Jetpack Compose
  frontend on `banana-ffi` is the next step; the FFI crate exposes library
  scanning and launching, and Kotlin bindings are generated with
  `uniffi-bindgen`.

Distribution resources are resolved from the executable's package marker;
development builds use the checkout layout. `just build` assembles an
allowlisted portable package in `target/dist` without local runtime data.
