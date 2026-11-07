# Minecraft Dungeons Arcade on Windows

The application runtime now uses a compiled Rust `banana-game-host` through
`banana-wsl`; build optional guest components with `just build-wsl`. The same
Rust compatibility crate handles Linux and WSL. The licensing investigations
below are historical; current seeded-session notes are in [Minecraft on Linux](minecraft-linux.md).
No Python or shell interpreter is required by the runtime.

**Current direction:** WSL2 remains a Windows backend option. Development of
the separate native Linux translation engine is paused at the user's request
while work focuses on Minecraft through WSL2.

**Latest measured failure:** the supported game's actual HASP API returns
status **7**, `HASP_HASP_NOT_FOUND` (protection key not found), captured before
the configuration-dependent logger crashes. The game's wrapper reports code 4;
its underlying API status is 7. No matching Windows Sentinel/HASP service or
device was detected, and localhost TCP 1947 was unavailable.

Diagnostics now execute a bounded, read-only GDB status capture instead of
blocking solely because the asset archives are protected. Ubuntu requires GDB
for this diagnostic. See `runtime/minecraft/license-probe.log` and the
`licensing` field in `report.json`. A successful diagnostic is not a claim of
gameplay. The normal runner never replaces the key check with a success result.

Banana pepper builds natively on Windows x86-64. This game's executable is
Linux x86-64 ELF, so its guest process runs in WSL2/Ubuntu and its window uses
WSLg. No TeknoParrot code, profiles or executable are used.

A separate test copy skipped the login check and retained the asset-seed
pointers. It still failed to read the PROGRAM/SCENE archives and localization,
then crashed during cleanup. That failed patch is excluded from the launcher.
The bundled utilities retain the same encrypted seed tables; no usable decrypted
seed state has been recovered. Minecraft is **not playable** with the current
implementation.

## Commands

Build with a current Rust MSVC toolchain and Visual Studio C++ build tools:

```powershell
cargo build -p banana-desktop
```

Pass either the actual game directory or a library root containing
`Minecraft Dungeons Arcade` (or the older `linux based/Minecraft Dungeons Arcade`). Named download folders containing
Minecraft and Dungeons are also discovered one directory level below the root.

```powershell
.\scripts\run-windows.ps1 -GamesRoot 'C:\path\to\Minecraft Dungeons Arcade'
.\scripts\run-windows.ps1 -Mode setup -GamesRoot 'C:\path\to\Minecraft Dungeons Arcade'
.\scripts\run-windows.ps1 -Mode probe -GamesRoot 'C:\path\to\Minecraft Dungeons Arcade'
.\scripts\run-windows.ps1 -Mode launch -GamesRoot 'C:\path\to\Minecraft Dungeons Arcade'
```

The corresponding executable arguments are `--games-root`,
`--setup-minecraft-libs`, `--probe-minecraft`, and `--launch-minecraft`.
Set `ARCADE_WSL_DISTRO` if the default WSL distribution is not Ubuntu.
Ubuntu needs Python 3, Bubblewrap, apt-get and dpkg-deb. Setup extracts the
distribution's GLU/LAPACK/BLAS/Fortran/wireless/PCSC packages into the project's
`runtime/minecraft/libraries`; it does not use sudo or change system packages.

## Local runtime libraries

This supplied folder has no FMOD Linux runtime libraries. The executable
requires **x86-64 FMOD 1.10** (`libfmod.so.10` and `libfmodstudio.so.10`).
The setup command now stages these from the archived FMOD 1.10.20 Linux SDK
published in the Manualinux package archive on SourceForge:

<https://sourceforge.net/projects/manualinux/files/Doomsday/fmodstudioapi11020linux.tar.gz/>

The archive is pinned to SHA256
`53231260b694286807995714bb11011afe08361eb4873bc75cf335873e167137`.
Only the two x86-64 runtime libraries and their license are extracted to
`runtime/minecraft/lib/`. Provenance and file hashes are saved in
`runtime/minecraft/fmod-source.json`. The SDK and its libraries are local
runtime files, excluded from source control and Banana pepper distributions. Windows
DLLs and newer FMOD major versions do not satisfy this ABI.

## Isolation and current limitations

Execution is restricted to the measured `gamedump.exec` SHA256:

`663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b`

This is an already modified executable supplied in the folder. Its GOT retains
stale library addresses from the machine where it was dumped. Banana pepper rebuilds
659 import relocations from the named records retained in the snapshot, adds a
data LOAD segment for normal ELF linker metadata, and lets the Linux linker
resolve the imports. Game instruction bytes and security checks remain
unchanged in this normal runtime copy. The Rust synthetic-image regression test
validates this against a local supplied snapshot without shipping the binary.

The supplied `game` file was also tested in a separate copy and exited with
code 1. `Patch`, `rtinit`, `go.sh`, and `run_game.sh` are never executed. Their
cabinet boot logic includes reboot and host configuration operations
inappropriate for a desktop.

Bubblewrap supplies `/pm/g7/minecraft` inside a private filesystem and process
namespace. Assets are read-only, networking is isolated, and only
`runtime/minecraft` is writable. The original pmuser tree is copied once for
runtime saves. Original files remain unchanged. The display and audio sockets
are exposed for desktop output.

`runtime/minecraft/report.json`, `dependencies.log`, and `runner.log` record
the startup result. A probe that survives 25 seconds is explicitly not evidence
of playable gameplay. Cabinet joystick mapping, card readers, security hardware,
and full gameplay have not been implemented or verified for this title.

## Measured remaining blocker

All shared libraries now resolve, including FMOD. With the reconstructed imports,
the game reaches cabinet dongle initialization, then fails in an error logger
that expects its configuration to be initialized (SIGFPE).

A separate, temporary investigation skipped the cabinet initialization block.
That test reached SDL/OpenGL initialization using Mesa llvmpipe, but logged
`Failed to open archive PROGRAM`, `Failed to open archive SCENE`, and
`localizationInit() failed!`, then crashed during cleanup. The program/scenes ZIP
files have encrypted/protected contents rather than ordinary ZIP headers; the
version and configuration files also yield unreadable data when opened without
their decoder. This is consistent with the supplied Sentinel/HASP protection
and `eFileMaps`, but a working data decoder/key has not been recovered.

Skipping the cabinet check therefore does not make the game work. That
unsuccessful instruction patch is not applied by the normal launcher. Normal
diagnostics now report `protected_asset_decoder_required` and avoid repeatedly
launching a known crash. The investigative trace is retained locally as
`runtime/minecraft/gdb-desktop-asset-failure.log`.

The FMOD issue is resolved; **Minecraft remains unplayable** in Banana pepper until
protected asset decoding and the cabinet interfaces are implemented and tested.

### Further asset-reader investigation

The native reader uses 128-byte and 64-byte seed tables. Its filename mixer was
reproduced and matched byte-for-byte against keys/IVs captured from the native
routine at `0x789830`. Comparing OpenSSL output with the native reader confirms
AES-256-OFB for the first file map. Assigning the compiled seed-table addresses
while preserving normal filesystem-table initialization still produces invalid
file-map plaintext. Canonical filename variations did not resolve that result.
The seed initialization path calls the statically linked HASP decryption API;
using the compiled ciphertext tables directly does not replace that operation.

The protected `game` executable's supervisor was also traced without GDB. Its
IPC and mutual ptrace operations succeeded with process-local tracing
permission, but its protected entry still exited with code 1. No system ptrace
settings were changed. This investigation did not produce a working seed pair
or a playable game. Temporary startup patches have again been removed from the
normal runtime image. Local evidence is in `asset-init.log`, `map-native.bin`,
`envelope-calls.log`, and the updated `report.json` under `runtime/minecraft`.

### Requested key-bypass test

A separate `game-key-bypass-test` copy skips the cabinet login/ID block and
preserves both asset seed-table pointers. The test gets past the missing-key
check, but still logs PROGRAM and SCENE archive failures, an unreadable game
version, and `localizationInit() failed!`, then crashes at `0x937c2b` during
cleanup. It does not decrypt the seed-table contents and does not produce a
playable game. The normal runtime image is hash-verified unchanged. The exact
result and source/patched hashes are in
`runtime/minecraft/key-bypass-result.json`. This unsuccessful patch is not
enabled by the normal launcher.
