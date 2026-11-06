# Big Buck Hunter Pro on Windows

**Water Adapts The ELF Runtime** is the name of the Linux-to-Windows layer.

Windows now offers two independent execution choices. **Automatic** selects the
Water worker; **WSL2 / WSLg** uses the existing Linux adapter. Choose in
Big Buck's Settings, or set `ARCADE_BBH_BACKEND=water` / `wsl` for one invocation.
Minecraft continues to use its existing WSL runner.

```powershell
rustup target add i686-pc-windows-msvc
cargo build -p arcade-water --bin water-buck --target i686-pc-windows-msvc
cargo run -p arcade-desktop -- --launch-big-buck "C:\path\to\Big Bug Hunter Pro"
# Build and run through the helper:
.\scripts\run-windows.ps1 -Game big-buck -Backend water -Mode launch -GamesRoot "C:\path\to\Big Bug Hunter Pro"
```

The game's folder spelling is preserved. Direct game folders, Downloads-like
parents and the old `linux based/Big Bug Hunter Pro/` library layout all work.
The executable must match the existing SHA256-verified Big Buck build. No source
file is altered; saves and logs are under `runtime/big-buck-native/`.

The worker is a separate **32-bit Windows executable**. It loads the reconstructed
ELF32 at its fixed Linux addresses, resolves ELF i386 relocations, and translates
the measured imported APIs to Windows. It does not use WSL, Wine, Linux shared
libraries, or a third-party loader. OpenGL/GLU calls cross a cdecl/stdcall bridge;
GLUT is implemented with a Win32 window/event loop; pthread mutex/thread requests
use Windows primitives; the game's own S16LE stereo 24000 Hz mixer feeds waveOut.
Linux random-number sequences, time structures, binary file modes, replace-on-
rename semantics and writable cabinet save paths are adapted explicitly.

Ordinary sessions have no automatic input or timeout. Mouse aims, left click
fires, right click pumps; T is Test, 1/Enter starts, 5/6 inserts coins, and Escape
or window close exits. The initial calibration can be cancelled with T, then 1
exits tracking. The supplied saves require four quarter-credit pulses for $1.
Use the in-game operator settings for free play if desired. Controls, resolution,
fullscreen, VSync, GPU/software OpenGL and audio enable are in Settings. Windows
uses its default audio output; Pulse sink selection belongs to the WSL backend.
Mouse coordinates scale to the 640×480 game space. Losing focus releases switches.

The bounded native progression probe is:

```powershell
cargo run -p arcade-desktop -- --probe-big-buck "C:\path\to\Big Bug Hunter Pro"
```

It opens a visible game window for 96 seconds, uses a separate
`runtime/big-buck-native-probe/` save directory, and captures checkpoints.
Test probes are muted. Both local game profiles are currently set to audio off
at the user's request; ordinary-session audio can be enabled again in Settings.
Observed Water results include calibration/tracking, attract mode,
four coins producing $1, Start/menu selection, a live rendered hunt, and a return
to site selection. Mouse shots and pump input have also produced a confirmed
doe hit and the game's penalty/game-over flow. Non-silent PCM was submitted successfully to Windows audio.
Complete scored hunts, full adventures, two players, physical guns, long-session
stability and listening confirmation remain separate validation requirements.
The probe's timeout is a bounded observation, not proof of gameplay success.

Regression checks include the Linux RNG sequence and actual calls through the
32-bit GLU bridge with double arguments and matrix pointers. To run worker tests
despite the freestanding DLL target's panic setting:

```powershell
$env:CARGO_ENCODED_RUSTFLAGS = '-C' + [char]31 + 'panic=unwind'
cargo test -p arcade-water --bin water-buck --target i686-pc-windows-msvc
Remove-Item Env:CARGO_ENCODED_RUSTFLAGS
```

This is a measured per-game compatibility worker, not a general-purpose Linux
distribution, CPU emulator, or security sandbox. Its legacy game mappings need
writable executable pages. Linux signals, arbitrary ELF32 libraries and unknown
builds are not supported. macOS/Android and non-x86 CPUs still need host/CPU and
graphics implementations plus device testing.

## Optional WSL backend

Use a WSL2 Linux distribution with WSLg, Bubblewrap and a 32-bit Linux userspace.
On Ubuntu, the required packages include `gcc-15-multilib` (or the distribution's
compatible multilib compiler), `libglut-dev:i386`, `libglu1-mesa-dev:i386`,
`libsdl1.2-dev:i386` and `libpulse0:i386`. Build the Linux launcher separately:

```sh
bash scripts/build-big-buck-wsl.sh
```

The script uses an installed Linux Rust toolchain, or the local toolchain under
`runtime/wsl-toolchain/` if present. The launcher goes into `target/wsl/debug/`
and the i686 guest shim into `target/i686-unknown-linux-gnu/debug/`. The standard
Linux desktop runner still uses read-only asset mounts and separate saves.
Private Xvfb probes additionally require Xvfb, strace, xdotool and ImageMagick.
Set `ARCADE_WSL_DISTRO` to choose a non-default distro.

On this Windows host, Ubuntu 26.04 built both artifacts and opened Big Buck's
calibration window through WSLg; the shared Linux adapter and non-silent desktop
audio were also observed. The short WSL verification was bounded to 45 seconds.

API references: [Windows rendering contexts](https://learn.microsoft.com/en-us/windows/win32/opengl/rendering-contexts)
and [waveOut buffer lifecycle](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-wavehdr).
