# Big Buck World 1.20

The supplied `game` is a decoded i386 Linux executable, SHA-256
`e1f17750ad82573a91cf3f2bbc27a14ca1573115ae8d8c515aa7f89f5571abd9`.
Other executable builds are rejected before patching. Keep `bbwdata/`,
`bbwuser/` and the supplied `hasp/` directory beside it. Neither the game,
its response corpus nor captures are distributed with Banana Pepper.

Build with `just build`. Prepare the pinned per-user C++ runtime once:

```text
arcade-desktop --action big-buck-world setup "<World folder>"
arcade-desktop --action big-buck-world launch "<World folder>"
```

The setup downloads Debian's checksum-pinned i386 libstdc++5 and copyright
notice into per-user dependency storage; it installs no system package.
Linux also uses the existing `legacy32` Theora dependency shared with Pro.
The registered source roots accept the game folder itself, a library folder
containing `Big Buck World (1.20)` or `Big Buck World`, and ZAR sources.

The cabinet verifies its resources at startup. Software rendering can spend
several minutes compiling shaders on first use; a black loading screen can occur
during that compilation.

Keyboard/mouse controls match Pro: T = operator test, 1/Enter = player one
start, 2 = player two start, 5/6 = coins, 9 = service, mouse = aim,
left click = trigger, right click = pump, Esc = exit. Controller bindings,
two-player aiming and crosshairs use `banana_input::session::Players`.

## Compatibility changes

- `prepare.rs` verifies the executable and expected instructions, makes its
  code segment writable in the generated view image, and replaces the cabinet
  ptrace supervisor with the game branch. The session owns process supervision.
- World's guest crate reuses Pro's JAMMA, GLUT and platform input implementation.
  World declares its own four hook addresses and window title. HASP hooks are
  World-specific and validate their expected instructions before writing.
- The supplied HASP memory dump and response files are read through the view.
  Response filenames encode the first 32 request bytes; the response must have
  exactly the requested length. Asset-key derivation uses these files on both
  encrypt and decrypt calls. Missing responses report an error.
- World encrypts live structures and performs integrity round trips over arbitrary
  graphics buffers. The adapter uses a reversible local byte transform and records
  produced block signatures so a game's first decode preserves preinitialized data.
  The integrity worker's scratch buffers use the transform in either direction;
  asset-key requests and startup challenges require exact supplied responses.
  Audit/save serialization writes through the VFS overlay. No response payload or
  cabinet key is embedded in the adapter.
- The read-only asset mount is `/g3/bbwdata`; `/bbwuser` maps to the writable
  overlay. Shared OSS playback preserves a device while muted and supports
  POSIX cancellation of the game's playback thread.

## Verification

The Linux mounted session has passed startup, resource verification and desktop
PulseAudio/PipeWire connection checks. Menu, gameplay and save checks are recorded
below as they are completed. Windows/Water and Android are not gameplay-validated;
compiling the adapter does not establish those platforms work. Android additionally
needs the World C++ dependency packaged in `opt/arcade/dependencies/big-buck-world`.
