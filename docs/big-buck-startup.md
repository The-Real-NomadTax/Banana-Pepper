# Big Buck Hunter Pro: first working startup

The supplied build now boots past calibration, reaches attract mode, accepts START and mouse shots in the game/mode/adventure menus, and reaches Whitetail Adventure instructions, Player 1 site selection, and a rendered hunt scene. Full scored gameplay and high-score saving are now confirmed; audio playback has an OSS-to-Pulse bridge. The test runs on a private 1024×768 Xvfb display for up to 45 seconds, with a 53-second outer timeout, and saves checkpoint images along the way.

## Interactive desktop mode

```sh
cargo run -p arcade-desktop -- --launch-big-buck games
```

Or select Big Buck Hunter in the workbench and click **Play on my display**. The game opens a normal 640×480 window on the current X11/XWayland display. It runs until you close it or press Esc, with no scripted inputs or timeout. Keep the launcher/workbench running while playing.

Controls: T = TEST, 1/Enter = left START, 2 = right START, 5/6 = coin switches, 9 = service, mouse = aim, left click = fire, right click = pump. At the initial calibration screen, T cancels into tracking and 1 exits tracking. The cursor remains visible and pointer coordinates scale to the game's 640×480 space if the window is resized. Keyboard/mouse buttons require game focus.

Desktop mode uses `runtime/big-buck-desktop/` for its own saves and `runner.log`. It does not overwrite the private test's save state. Original game assets stay read-only in Bubblewrap; the current X11 socket and Xauthority file are exposed to the guest for interactive display access. This mode was explicitly requested by the user after the earlier private-display test. It requires no Xvfb installation, strace, xdotool or ImageMagick. The private probe remains available separately.

The shim redirects a cabinet-supplied `-display :0.0` argument to the current DISPLAY during GLUT initialization. A normal desktop launch and visible game window were verified on the current host. Audio playback is bridged to PulseAudio/PipeWire; the game fully loads into scored gameplay and saves scores correctly.

## Private progression test

From the project root:

```sh
cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu
cargo run -p arcade-desktop -- --probe-big-buck games
```

Or start the workbench with `cargo run -p arcade-desktop`, select Big Buck Hunter Pro, and click **Test Big Buck Hunter progression**. The workbench displays the resulting still frame. The test automatically sends TEST, START, COIN and a fixed sequence of mouse shots to the private game window. The resulting still frame is not an interactive game view.

Requirements: Linux x86 with 32-bit glibc, GL/GLU/FreeGLUT and software Mesa; Rust's `i686-unknown-linux-gnu` target and a 32-bit linker toolchain; Bubblewrap, Bash, GNU timeout, strace, Xvfb, xdotool and ImageMagick `import`. The tested host already had the 32-bit game libraries. Xvfb was downloaded from the configured distribution mirror and extracted locally under `runtime/tools/xvfb`; no system package was installed. Other distributions will need their matching packages, not that CachyOS binary.

Outputs are in `runtime/big-buck/`:

- `screen.png`: captured private display.
- `result.json`: source/runtime hashes, window-detection result and game exit status.
- `stdout.log`, `stderr.log`, `startup.strace`, `xvfb.log`, `runner.log`: diagnostics.
- `game`: prepared runtime executable.
- `bbh3user/`: persistent copy of the supplied save/audit data, seeded without replacing existing runtime saves.

Exit code 124 in the game status means the startup test reached its timeout. The runner reports window creation separately; a process exit code alone does not establish game success.

## Changes required

The original file is 51,469,664 bytes, SHA256:

```
c569677c4e4bcf3449939b6e9a44afd3346c153133391c24dc06c012a58c1244
```

Native startup of the original failed at `open("/dev/rockey", O_RDWR)`, printing `please install the key driver!` and exiting with code zero. Static examination showed a startup wrapper with a repeating four-byte XOR transform over one code region. The reconstructed entry decodes to the standard i386 CRT startup sequence and calls the game main routine.

`arcade-core::big_buck::prepare` accepts only the hash above. It reconstructs bytes at file offset `0x2d80`, length `0x937c0`, using mask `84 eb 67 f1`; restores the ELF entry to `0x0804ad80`; and clears reserved ELF identification padding. All changes go to a separate runtime copy. Unknown executable hashes are rejected. The runtime copy is not an authenticated original; its behavior is validated only to the startup screen so far.

The first graphics attempt failed because FreeGLUT cabinet game mode tries to change screen settings. Our guest-side Rust `cdylib` overrides `glutEnterGameMode`/`glutLeaveGameMode` to create and destroy a normal 640×480 GL window. It is built for i686 and preloaded only into the guest process. Software Mesa is selected for both Xvfb and the game to avoid a host NVIDIA EGL initialization crash.

## Private test isolation

Bubblewrap exposes system libraries and game assets read-only, a private `/tmp`, fresh `/dev` and `/proc`, and a writable runtime directory. The original game directory is not writable. A private Xvfb server is started inside the same namespace. No host X11 socket, home directory, authentication cookie, GPU device, serial port or network namespace is exposed. The test has an outer timeout and child-process cleanup.

Automatic approval review rejected an earlier proposal to connect the reconstructed game to the live desktop X11 server. The implemented backend uses the isolated display instead. The screenshot can be viewed in the trusted egui frontend without exposing the host desktop to the game.

## Cabinet input adapter and current limits

The game repeatedly opens `/dev/ttyS0`, then logs:

```
JammaOp(JAMMAOP_RESET) failed -100
JammaLoop() failed -9098, resetting jamma
```

The new guest shim replaces four internal functions in memory, after build validation, with an experimental **game-side JAMMA API adapter**. This avoids opening the absent serial board; it is not a wire-level serial implementation. The observed function addresses for this build are Init `0x080d9ce5`, Close `0x080d9e81`, Loop `0x080dbf32`, and variadic Op `0x080da943`. All four prologues are checked before any hook is written. The adapter is enabled by `ARCADE_BBH_ADAPTER=1` and installed before game initialization. Runtime executable bytes on disk retain the prior prepared hash.

The game's switch table maps `g11`, `st1`, `cn1`, `srv`, and `tst` to monotonic transition counters. The adapter counts press/release edges, suppresses repeated key-downs, returns a stable counter buffer for Op `0x12`, and acknowledges the dirty flag when requested. A small assembly trampoline preserves the original cdecl variadic stack layout and aligns the stack before entering Rust. Op `6` stores the game's callback, used to deliver aim and trigger payloads on the GLUT/game thread. Callback tracking lists are empty; real optical tracking is not simulated.

Key mappings in the private game window: `T` = TEST, `1`/Enter = left START, `2` = right START, `5`/`6` = coin switches, `9` = service, Space = left trigger switch, left mouse = left gun shot plus trigger switch, right mouse = left pump. Coin-switch delivery is logged, but the supplied saved configuration starts in FREEPLAY, so paid credit accounting is not verified. Physical guns are not supported.

Two players (`players = 2`): controller 2 is the second gun. The game's switch table orders the guns as `g11`, `g21`, `g12`, `g22` (indices 1–4): gun one's trigger and pump are switches 1 and 3, gun two's are 2 and 4. The gun callback (`0x0804d9f0`) takes the event type in the low byte and the gun number in bits 8–11, so gun two's moves and shots are `0x109` and `0x101`. Controller 2's Start and Coin press Start 2 and Coin 2. The shim draws gun two's crosshair in the other colour. These were read from the binary and are covered by unit tests; a two-gun hunt has not been played.

The adapter reports a virtual connected board/version. A limited set of observed configuration/output operations return success without implementing physical effects; unimplemented operations return `-9000`. Gun calibration transformations, board firmware, lamps, and accurate hardware capability semantics are incomplete. Do not treat successful menu navigation as a faithful serial-board implementation.

Verified checkpoint captures: `tracking.png`, `after-start.png` (attract mode), `game-select.png`, `mode-select.png`, `adventure-select.png`, `adventure-intro.png`, `records.png`, `instructions.png`, `site-select.png`, and `hunt.png`. `screen.png` is the later capture shown in the workbench. These images document observed screens; the runner's JSON currently detects window creation only and does not automatically verify the screen contents.

A hunt scene renders after site selection, but a scored hit and correct pump/reload behavior have not been demonstrated. Interactive testing is available in the desktop launch mode. Gameplay timing, persistent audits, focus/disconnect releases, and clean game-initiated shutdown remain unverified. The former sound-load/player failures were eliminated in the audio test described below.

## Audio playback

`arcade-linux-shim/src/audio.rs` implements the subset of OSS playback used by this build. The game still performs its own asset loading and mixing. Its requests to open `/dev/dsp`, negotiate S16LE/stereo/24000 Hz, query fragment sizes/free space, and write PCM are translated into a blocking PulseAudio Simple API playback stream. PipeWire's PulseAudio server is supported through that same interface. The 32-bit `libpulse-simple` library and its dependencies are required.

The launcher mounts only the selected local Pulse socket (normally `$XDG_RUNTIME_DIR/pulse/native`) and an existing Pulse cookie when present. It sets `ARCADE_BBH_AUDIO=1` and directs the guest at that socket. The current default output device is used; `ARCADE_AUDIO_SINK` can optionally name another sink. If no local socket exists, the OSS bridge is not enabled. Desktop audio connection and PCM submission are recorded in `runtime/big-buck-desktop/runner.log`.

The audio descriptor is backed by a real private `/dev/null` fd. Reads/writes on all other file descriptors continue to use the kernel normally. Naked C ABI entry thunks retain the optional `open` mode and `ioctl` argument from the i386 caller's stack; ordinary asset files and logs must not be mistaken for sound data. Audio format and channel validation rejects unsupported requests. No mmap/duplex/trigger capability is advertised. Pulse writes provide playback pacing. Fragment-space reporting is a minimal compatibility model, not full OSS hardware timing.

Cabinet mixer writes are acknowledged without changing the host's master volume. Use your desktop's normal per-application/output controls; game-side cabinet mixer volume fidelity remains incomplete. Reset/flush and drain are supported. Device hotplug/reconnect, exact audio latency and long-session behavior have not been validated.

Verification: the private progression test was routed to a temporary null sink and that sink alone was captured to `runtime/big-buck/audio-test.wav`. The capture was stereo 16-bit 24000 Hz, 1,277,952 frames (53.248 seconds), with a peak sample of 16,289 and RMS 1,632.06. The latest game session logged zero sound-load failures, zero no-free-player errors and zero unsupported audio ioctls. The temporary sink was then removed. The game was relaunched on the normal desktop output and its named playback stream was confirmed active and unmuted.

Native i686 tests cover OSS negotiation, null argument rejection, unsupported format rejection, the variadic ABI, and unaffected ordinary file creation/write/close behavior. API reference: [PulseAudio Simple API](https://www.freedesktop.org/software/pulseaudio/doxygen/simple.html).
