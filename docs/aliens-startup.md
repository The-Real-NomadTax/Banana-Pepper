# Aliens Armageddon desktop runtime

Status: Aliens opens on the user's display and reaches playable 3D gameplay. The user confirmed playing successfully with working controls. Native desktop mouse maps are enabled, with keyboard **1** for Start and **5** for coin. Audio remains disabled. A complete playthrough, all weapons, player two and physical cabinet hardware are not verified.

## Launch

From the project root:

```sh
cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu --offline
cargo run -p arcade-desktop --offline -- --launch-aliens games
```

The egui workbench also has **Open Aliens on my display**. Click the game window, press **1**, then use the mouse and left click to choose a game mode. **T** opens the operator test menu (T select, 9 back, Up/Down move). **Esc** or the window's close button quits. Keys are customizable in the workbench or `controls.cfg`. The copied cabinet settings currently select Spanish: **MODO HISTORIA** is Story Mode. Basic desktop aiming and firing were confirmed by the user. Additional control configuration and audio can follow separately.

The launcher uses Bubblewrap with the current desktop X11 socket/GPU, read-only original assets and separate writable saves. Cabinet resolution-switching, reboot and shutdown helper commands resolve to inert private scripts. The supplied cabinet launch/update scripts, third-party loader files and `patch` executable are not executed. No TeknoParrot implementation is used.

Required host components include the 32-bit Linux/GL libraries, the staged compatibility libraries under `runtime/jurassic/libs`, Bubblewrap, Python, Bash, xdotool and ImageMagick. This is currently a Linux x86 backend, not CPU emulation or support for Windows/macOS/Android hosts.

Original game SHA256: `297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db`.
Prepared runtime SHA256: depends on `controls.cfg`. The Rust preparer writes it to `runtime/aliens/game-native.sha256`, and the harness refuses any other image. Before customizable controls, the verified image was `9bd2332bb9c49adb5a3f0854c8c52532f0c3ab29877cee2dae9eb7c8371c5d61`.

## Compatibility changes

The Rust core validates the original executable hash and prepares a separate runtime image. It removes unused ESD and absolute CSV dependency declarations; our Rust shim supplies `csv_write2`. Desktop launches select the game's built-in SDL `pulse` audio driver with only the Pulse/PipeWire playback socket exposed; probes use `dummy`. `ARCADE_AUDIO=0` forces silence.

The runtime copy omits the cabinet's fork/ptrace supervisor, which conflicts with desktop diagnostics and can invoke an audit error handler before asset/type initialization. The normal child continuation is retained and the desktop harness supervises the process.

The original G6 decoder initially fed invalid data into zlib and failed all six production type-definition headers. Two supplied `hasp/` records contain decoded asset seed tables. The preparer verifies their sizes and exact SHA256 values, then restores those tables in the runtime copy:

| Supplied record | Bytes | SHA256 |
| --- | ---: | --- |
| `2a3ccf10ef8dff97539ed4b3cd113701` | 512 | `369ff9829782a47ad878183bb26fae5c5566923c23a1d58d237ef435487756b8` |
| `91a0dda258cc5df430a5678cc72cd603` | 256 | `411f22ab8a0bb1ead127f7206e7668ce88ad1155fc61ca39b40e065eb3a561dc` |

The game's own filename mixer, AES reader and decompressor then load the production headers, configuration, shaders and game resources. No plaintext directory substitution is needed for normal launch. Dongle initialization still reports failure in the native log; this backend does not implement or claim complete dongle support.

The runtime retains the requested 1360×768 size while forcing an ordinary desktop window. The Aliens-only Rust GLX adapter routes old SGI swap-interval calls through the current-context-aware EXT interface, avoiding NVIDIA BadValue errors from negative/pre-context requests. Other profiles continue forwarding their original SGI calls.

Two native flag-reset sites select the engine's existing desktop input mode. This supplies native mouse maps and allows Start without attached cabinet guns. The engine registers cabinet switches with an add-map routine (`0x080b98c0`, table `0x0891f710`, count `0x08921710`; the same source/target enums as Jurassic). Sixteen `mov [esp], source` operands in the two input-setup routines are rewritten to the SDL keys from `controls.cfg`:

| Cabinet switch (source) | Targets | Default key |
| --- | --- | --- |
| P1 Start `0x154` | Start 1, menu confirm | 1 |
| P2 Start `0x155` | Start 2, menu confirm | 2 |
| Test `0x150` | Test menu `0x17a`, menu select | T |
| Service `0x151` | Service `0x179`, menu back | 9 |
| Volume up `0x152` | Volume +5, menu up | Up |
| Volume down `0x153` | Volume −5, menu down | Down |
| Coin 1 `0x14c` / Coin 2 `0x14d` | Coins | 5 / 6 |

Test, Start 1 and Start 2 are also re-registered by a second setup routine at `0x08193dba`; those sites use the same keys. Test, Service, Volume and P2 Start were added with customizable controls and still need in-game confirmation. Every executable patch site is fingerprint-checked before writing; original game files remain unchanged.

## Quitting

The engine ignores SDL quit requests, and its statically linked SDL loads X11 itself, so the Jurassic-style SDL event hook cannot be used. The launcher gives the window the unique class `BananaPepperAliens` (`SDL_VIDEO_X11_WMCLASS`). A shim thread (`aliens_exit.rs`) then watches it on its own XCB connection:

- **Esc** (keycode 9) while an Aliens window has keyboard focus ends the process. Esc pressed in other windows is ignored.
- WM_DELETE_WINDOW is removed from the window's `WM_PROTOCOLS` (re-checked every second). The **close button** then makes the window manager disconnect the client instead of sending a request the game ignores. The resulting Xlib "connection broken" exit is reported as a normal close (`closed_by` in `result.json`).

XCB is loaded with `dlopen` and reports errors per request, so the game's Xlib error handlers are never touched. This was tested against Xvfb with SDL-shaped windows and real XTest key presses. It has not yet been confirmed with the game on the user's desktop.

## Verification and diagnostics

Verified: rendered attract scenes, mode selection and active gameplay on the desktop. Captures show the health/ammunition HUD, enemies and increasing kill totals; the user explicitly confirmed playing and that controls/visuals work. The session was left running. `runtime/aliens/gameplay-verification.json` records this manual confirmation separately from automatic startup classification.

The original first-probe black-window/null-configuration failure is resolved. Individual captures during transitions can still be black. The supplied/copied save settings use Spanish. Audio quality, advanced control behavior, player two, a full playthrough and cabinet-device protocols remain unverified.

```sh
cargo run -p arcade-desktop --offline -- --probe-aliens games
python3 scripts/aliens-run.py --games-root games --debug
```

The normal probe times out after 45 seconds and records startup logs/captures under `runtime/aliens/`. Automatic startup output does not certify gameplay. Debugger mode stops at a decompression breakpoint; `--plaintext-assets` is an optional diagnostic overlay, not the normal runtime.
