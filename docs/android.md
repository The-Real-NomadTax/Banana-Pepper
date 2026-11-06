# Android: Big Buck Hunter Pro

Banana pepper has an Android app (`apps/android/`) that runs the supplied 2008 Linux build of Big Buck Hunter Pro on ARM64 phones. Gameplay was verified on a Samsung Galaxy S24 FE (SM-S721U, Exynos 2400, Android 17, ARM64 only with no 32-bit support): attract mode, game and mode menus, a Whitetail hunt with scored shots, and initials entry. Audio reaches an active Android AudioTrack. Audibility, long sessions and other devices are not yet verified.

## Using the app

1. Install `BananaPepper.apk`. The first launch unpacks the game runtime, which takes a few seconds.
2. Tap **Import game folder…** and choose your own `Big Bug Hunter Pro` folder, the one containing `game` and `bbh3data` (about 640 MB). The app copies it into private storage and checks that `game` is the supported build (SHA-256 `c569677c…`). Files placed in `Android/data/com.bananapepper/files/games/` (for example with `adb push`) are also found, and are used in place.
3. Tap **Play**. The game screen is landscape:
   - Tap the picture to aim and fire. A held finger can be dragged to aim.
   - Pump or reload: the **PUMP** button, a second finger, or a tap beside the picture.
   - **COIN**, **START**, **P2 START** and **TEST** are the cabinet switches. At the calibration screen, **TEST** skips it.
   - **EXIT** or the Back gesture quits.
4. **Sound** on the main screen turns audio on or off. **Show last game log** shows the runtime logs.

## How it works

```
Android app (Java)                      proot (Debian trixie arm64 rootfs)
 MainActivity: runtime install,          run-big-buck.sh
   import, hash check, prepare            ├─ Xvfb :0 640x480 (llvmpipe GLX)
 GameActivity ──touch──► abstract ──────► ├─ afbridge: XTest input in,
   GameView ◄── mmap frame file ◄────────┤    screen copy out (60 fps)
   AudioBridge ◄── abstract socket ◄─────┘─ box64 (BOX32) /game/game
                       (S16LE PCM)              + LD_PRELOAD guest shim
```

- **CPU:** box64 built with `BOX32=ON` translates the i386 game. It wraps the native arm64 libc, X11, GL, GLU, SDL and Pulse libraries; freeglut and libgcc are emulated i386 copies. The phone has no AArch32 support, so box86 is not an option.
- **Graphics:** Mesa llvmpipe renders into Xvfb. `afbridge` copies the screen into `files/run/frame.bin` (32-byte header plus RGBA), and `GameView` draws it scaled to 4:3 with a hardware canvas. Android apps cannot open the GPU's DRM device, so rendering is software only.
- **Input:** the app sends `m x y`, `b N 0|1` and `k keysym 0|1` lines to `afbridge` over an abstract Unix socket. `afbridge` injects them with XTest. The existing shim turns them into JAMMA switch edges and gun callbacks, exactly as on the desktop.
- **Audio:** with `ARCADE_AUDIO_SOCKET` set, the shim's OSS device streams PCM to the app over an abstract socket instead of using PulseAudio. A small send buffer and the blocking `AudioTrack` writes pace the game.
- **Exec permission:** the app targets API 28, as Termux does, because Android only lets apps execute binaries from their private files directory at that target level.
- **SELinux:** sockets are abstract because SELinux denies named FIFOs in some app and shell directories.
- **Lifecycle:** `run-big-buck.sh` kills any processes left by an earlier session and exits when the app process (`AF_APP_PID`) disappears.

### Compatibility fixes made for Android

1. **box64: section-less i386 ELF.** The prepared game has no section header table. box64's 32-bit loader only found `.dynamic` and `.dynsym` through section headers, so it crashed in `old_elf_lookup` with a null hash table. `apps/android/rootfs/patches/box64-sectionless-elf32.patch` recovers them from `PT_DYNAMIC`: `DT_SYMTAB`, `DT_STRTAB`, `DT_HASH`/`DT_GNU_HASH`, `DT_VERSYM` and `DT_PLTGOT`. It applies to box64 commit `59b3eb1`.
2. **Shim raw syscalls.** box32's `syscall()` wrapper returns `ENOSYS` for i386 `openat` (295) and `ioctl` (54). The shim now falls back to the next libc symbol (`RTLD_NEXT`) after the first `ENOSYS`, so desktop behaviour is unchanged. The fallback uses libc `open`, not `openat`, because box64 substitutes an x86 `/proc/cpuinfo` there. The game reads "MHz" from that file and crashes if it's missing.

## Building

Requirements: Android SDK (build-tools, platform 36+), JDK 17, Rust with the `i686-unknown-linux-gnu` target, and an arm64 phone with USB debugging.

```sh
apps/android/fetch-proot.sh               # proot for Android, from the Termux repo
apps/android/rootfs/setup-dev-device.sh   # one-time: Debian + box64 build on the phone
apps/android/rootfs/make-rootfs.sh        # build the trimmed runtime rootfs on the phone
apps/android/build-apk.sh                 # -> apps/android/build/BananaPepper.apk
adb install -r apps/android/build/BananaPepper.apk
```

The rootfs is built on the phone itself, through proot over adb, so the host needs no ARM toolchain or QEMU. `make-rootfs.sh` installs only runtime packages and removes the i386 Mesa and LLVM that package dependencies pull in, since box32 uses the native GL. The archive is about 127 MB. `TRIM_ONLY=1` re-trims and repacks without reinstalling. `build-apk.sh` builds a debuggable APK, so `adb shell run-as com.bananapepper` can read `files/run/*.log`. `RELEASE=1` builds a non-debuggable one. Signing uses `apps/android/debug.keystore`, which is generated on first build and not committed.

## Known limits

- Software rendering only. A 640×480 game window, scaled up on screen.
- Only the hash-verified build is accepted, as on the desktop.
- One player gun. Two-gun Shootout, physical light guns, lamps and calibration fidelity are not implemented.
- The game stops when the app is closed. If you switch away, the game keeps running until Android stops the app.
