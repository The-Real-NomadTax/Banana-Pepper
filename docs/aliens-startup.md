# Aliens Armageddon desktop runtime

Status: Aliens opens on the user's display and reaches playable 3D gameplay. The user confirmed playing successfully with working controls. Native desktop mouse maps are enabled, with keyboard **1** for Start and **5** for coin. Audio remains disabled. A complete playthrough, all weapons, player two and physical cabinet hardware are not verified.

## Launch

From the project root:

```sh
cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu --offline
cargo run -p arcade-desktop --offline -- --launch-aliens games
```

The egui workbench also has **Open Aliens on my display**. Click the game window, press **1**, then use the mouse and left click to choose a game mode. The copied cabinet settings currently select Spanish: **MODO HISTORIA** is Story Mode. Basic desktop aiming and firing were confirmed by the user. Additional control configuration and audio can follow separately.

The launcher uses Bubblewrap with the current desktop X11 socket/GPU, read-only original assets and separate writable saves. Cabinet resolution-switching, reboot and shutdown helper commands resolve to inert private scripts. The supplied cabinet launch/update scripts, third-party loader files and `patch` executable are not executed. No TeknoParrot implementation is used.

Required host components include the 32-bit Linux/GL libraries, the staged compatibility libraries under `runtime/jurassic/libs`, Bubblewrap, Python, Bash, xdotool and ImageMagick. This is currently a Linux x86 backend, not CPU emulation or support for Windows/macOS/Android hosts.

Original game SHA256: `297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db`.
Prepared runtime SHA256: `9bd2332bb9c49adb5a3f0854c8c52532f0c3ab29877cee2dae9eb7c8371c5d61`.

## Compatibility changes

The Rust core validates the original executable hash and prepares a separate runtime image. It removes unused ESD and absolute CSV dependency declarations; our Rust shim supplies `csv_write2`. SDL audio uses the dummy backend.

The runtime copy omits the cabinet's fork/ptrace supervisor, which conflicts with desktop diagnostics and can invoke an audit error handler before asset/type initialization. The normal child continuation is retained and the desktop harness supervises the process.

The original G6 decoder initially fed invalid data into zlib and failed all six production type-definition headers. Two supplied `hasp/` records contain decoded asset seed tables. The preparer verifies their sizes and exact SHA256 values, then restores those tables in the runtime copy:

| Supplied record | Bytes | SHA256 |
| --- | ---: | --- |
| `2a3ccf10ef8dff97539ed4b3cd113701` | 512 | `369ff9829782a47ad878183bb26fae5c5566923c23a1d58d237ef435487756b8` |
| `91a0dda258cc5df430a5678cc72cd603` | 256 | `411f22ab8a0bb1ead127f7206e7668ce88ad1155fc61ca39b40e065eb3a561dc` |

The game's own filename mixer, AES reader and decompressor then load the production headers, configuration, shaders and game resources. No plaintext directory substitution is needed for normal launch. Dongle initialization still reports failure in the native log; this backend does not implement or claim complete dongle support.

The runtime retains the requested 1360×768 size while forcing an ordinary desktop window. The Aliens-only Rust GLX adapter routes old SGI swap-interval calls through the current-context-aware EXT interface, avoiding NVIDIA BadValue errors from negative/pre-context requests. Other profiles continue forwarding their original SGI calls.

Two native flag-reset sites select the engine's existing desktop input mode. This supplies native mouse maps and allows Start without attached cabinet guns. Three source-map operands add keyboard Start, matching menu confirm and coin. Every executable patch site is fingerprint-checked before writing; original game files remain unchanged.

## Verification and diagnostics

Verified: rendered attract scenes, mode selection and active gameplay on the desktop. Captures show the health/ammunition HUD, enemies and increasing kill totals; the user explicitly confirmed playing and that controls/visuals work. The session was left running. `runtime/aliens/gameplay-verification.json` records this manual confirmation separately from automatic startup classification.

The original first-probe black-window/null-configuration failure is resolved. Individual captures during transitions can still be black. The supplied/copied save settings use Spanish. Audio quality, advanced control behavior, player two, a full playthrough and cabinet-device protocols remain unverified.

```sh
cargo run -p arcade-desktop --offline -- --probe-aliens games
python3 scripts/aliens-run.py --games-root games --debug
```

The normal probe times out after 45 seconds and records startup logs/captures under `runtime/aliens/`. Automatic startup output does not certify gameplay. Debugger mode stops at a decompression breakpoint; `--plaintext-assets` is an optional diagnostic overlay, not the normal runtime.
