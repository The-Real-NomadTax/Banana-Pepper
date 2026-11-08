> Research record: paths and launch transcripts below describe the earlier development harness. Current launches use the provider/mount/session architecture in architecture.md. Generated images are memory-backed; durable state uses per-user directories. Revalidate gameplay through the new pipeline before treating historical results as current support.

# Ford Racing Full Blown

The supplied EU 1.1.6 executable is SHA256
`ef7011d4c59bcf68cbbe8a3efd31443638089a96ac2dc1747d9aa3d912b41e76`.
Windows verification reached attract mode, vehicle/track selection and a live
race. The native game uses Direct3D 9 and DirectSound. The adapter supplies its
cabinet input report without a physical USB board or kernel port driver.

```powershell
rustup target add i686-pc-windows-msvc
cargo build -p banana-ford-hook --target i686-pc-windows-msvc --release
cargo run -p banana-desktop -- --launch-ford "C:\path\to\Ford Racing"
# Or:
cargo xtask run --action ford launch "C:\path\to\Ford Racing"
```

LLVM `lld-link` must be on PATH. Select **Ford Racing Full Blown → Play Ford
Racing** in the workbench. Inspection accepts the game folder, a Downloads-like
parent, or a library with `Ford Racing/` (or the older `windows based/Ford Racing/`).

Controls default to Left/Right steering, Up gas/menu confirm, Down brake,
Enter/1 Start, V view, Space/X shift up/down, 5 credit, and Escape exit.
Bindings are editable in Settings and saved in `controls.cfg`. Input is only
read while the game owns the foreground window. Steering returns to centre on
release. Physical wheels, force feedback and cabinet networking are unfinished.

The launcher copies assets and seed data to `runtime/ford-racing/`, preserves
runtime saves, checks the executable hash again, and installs our `inpout32.dll`.
The supplied GameShell, `dinput8.dll`, `JVSEmuFR.dll`, original `inpout32.dll`, and
TeknoParrot configuration are not loaded. Source files are not changed.
`arcade-ford.log` identifies the active adapter and logs switch edges/faults.
Do not launch the original cabinet shell or driver installers.

Resolution, fullscreen and sound enable/disable apply to the runtime copy of
`data/GameSettings.ini`. Native Windows defaults to a 640×480 window. The game
accepts 640×480, 800×600, 1024×768, 1280×1024 and 1600×1200. It pauses when unfocused.
Windows uses the game's default VSync and system sound output. Complete race
completion, manual gearing and long sessions require further validation.

On Linux x86-64, the same launcher and adapter are available through 32-bit Wine
using a private `runtime/ford-racing/prefix`. That new Ford/Wine path has not yet
been verified on this host. macOS and ARM hosts still need execution backends;
portable profiles and a reusable adapter do not automatically translate CPUs.
