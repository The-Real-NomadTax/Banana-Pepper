# Android: native Kotlin UI, shared Rust sessions

The Android frontend uses the same folder/ZAR providers, build identification,
compatibility plans, transforms, dirty-block overlays, entry identity and session
API as desktop. Kotlin owns the native library/game UI, touch input, frame display
and AudioTrack output. Rust owns preparation, execution and process cleanup.

Normal, unrooted ARM64 phones are the target. Android apps cannot mount an arbitrary
FUSE filesystem. Every guest process instead runs under `banana-tracer`
(`backends/android/tracer`), a Rust ptrace tracer that gives it the runtime
image as its root and brokers game paths into `banana-vfs` through a versioned
view protocol. No extracted game tree or patched executable is written to disk.

## Namespace and persistence

Filesystem metadata, enumeration, reads, writes, renames and deletion markers use
the shared Rust View. Ordinary I/O streams through that view. Sparse memfds provide
transient backing for mapped ranges and executable loading; opening a writable file
does not populate it. Shared mapping changes are checkpointed into the same dirty
blocks, and explicitly flushed on fsync, close and session shutdown. Private mappings
do not persist their private writes. Child processes inherit the namespace.

The current adapter allows up to 256 MiB of populated mapping/cache ranges per
session. Unsupported operations, including
resizing a file mapping with mremap and certain vector-positioned I/O calls, return
an explicit OS error rather than falling back to extracted files. These limits and
Android kernel behavior need game/device testing before gameplay is certified.

Configuration, durable data and per-entry state live under the app's private
`files/config`, `files/data` and `files/state` directories. Mount namespace entries
use the app cache. Imported sources remain source content; launching never stages
another game copy. Existing Android saves and operator settings are imported through
title-declared rules, without deleting old sessions or copying their assets.

Box64 executes Linux guests; Wine/Box64 executes supported PE guests. Per-title
adapters remain under `game-compat/`. Reusable box32/libc and pthread ABI fixes remain
in the generic Linux guest services. Minecraft and Skylanders remain excluded from
the Android registry until they have Android requirements and validation.

The tracer is one executable (`files/bin/banana-tracer`):
- **Guest root:** the runtime image as the root, with binds.
- **Programs:** guest programs run through their own `ld.so` or `#!`
  interpreter.
- **Root and links:** fake root, and hard links as symbolic links (Android
  denies link()).
- **Game files:** view protocol 2, with descriptor handoff (see the SM-S721U
  notes below).
- **Filtering:** a seccomp filter, so only the calls it handles stop the guest.
  Xvfb and PulseAudio never stop on reads and writes.

It replaced PRoot, its loader, talloc and libandroid-shmem. Sessions check the
marker its binary carries for the view protocol. A verbosity level in
`files/config/tracer-trace` logs execs (1) and translated paths (2) into the
game log. See the tracer's README for what it covers and what remains (group
stops).

## Build

Every step is a `cargo xtask android` command (`tools/xtask`). Outputs and
downloads go to `apps/android/build/` (or `BANANA_ANDROID_BUILD`). No phone,
emulator or shell script is involved:

```sh
cargo xtask android all          # every step this host can run, then the APK
cargo xtask android kotlin       # pinned Kotlin 2.2.21 compiler (SHA-256 checked)
cargo xtask android wine         # pinned Wine 11.0 WoW64 build for Windows titles
cargo xtask android x64-libs     # Debian x86-64 libstdc++/libgcc_s for Box64's Wine side
cargo xtask android box64        # patched Box64 with BOX32, cross-compiled
cargo xtask android rootfs       # Debian arm64 runtime image from packages
cargo xtask android resources    # crate-declared title adapters (Linux host)
cargo xtask android apk          # build/BananaPepper.apk, signed with the debug key
```

- **rootfs** reads Debian trixie's arm64 and i386 indexes, resolves the essential
  set plus the runtime packages (Depends and Pre-Depends, as apt
  `--no-install-recommends` would), downloads each `.deb` checked against its
  index SHA-256 over HTTPS, and merges the packages' data archives in memory.
  Nothing is extracted or run. The merge follows merged `/usr`, keeps only each
  package's copyright file under `/usr/share/doc`, and drops the i386 Mesa/LLVM
  that Box32 never loads. The few postinst results the runtime relies on are
  written directly (accounts, `/bin/sh`, nsswitch, alternatives such as `awk`,
  fontconfig defaults, UTC). The result is
  `build/rootfs.tar.gz` plus `build/rootfs.packages` (the package list).
- **box64** checks out pinned Box64, applies `apps/android/patches/*.patch`, and
  cross-compiles it with clang, lld, CMake and Ninja against a Debian arm64
  sysroot made from packages. It needs LLVM and Python 3, which Box64's own
  CMake requires, and uses the Android SDK's CMake and Ninja when present.
  Outputs are `build/box64` and `build/box64-install` (rc file and BOX32's i386
  libraries).
- **resources** needs a Linux host (WSL works). The title adapters link against
  i686 GL/GLUT, SDL 1.2 and PulseAudio development libraries, the same as the
  desktop Linux adapters.
- **apk** runs on Windows, Linux or macOS with the SDK, an NDK built for that
  host, and a JDK. It builds the JNI library and `banana-tracer` for
  aarch64-linux-android, and checks the tracer's protocol marker. Assets,
  native libraries and dex are appended to aapt2's base APK, because aapt2 on
  Windows names nested assets with backslashes. The APK is signed with the local
  debug key (`apps/android/debug.keystore`): install it by allowing installs from
  unknown sources. Optional legacy libraries come from `BANANA_ANDROID_LEGACY32`
  (default `~/.local/share/banana-pepper/dependencies/legacy32`; `just
  setup-libraries` fills it on Linux).

`cargo xtask android input PORT FILE` replays transport input lines (`m x y`,
`b n 0|1`, `k key 0|1`, `@ms` pauses) through
`adb forward tcp:PORT localabstract:<input socket>`.

For the earlier `com.arcadefoundry` installation, build with
`APP_ID=com.arcadefoundry` and retain its signing key. Back up app data before
updating a development device, and install with
`adb install --no-incremental -r apps/android/build/BananaPepper.apk`.
On the SM-S721U, an incremental update reported success before Android removed
the package and its data; the streamed installation succeeded.

The APK packages the ARM64 JNI library, `banana-tracer` and the crate-declared
title artifacts. The JNI library retains 16 KiB load alignment. A runtime whose
tracer lacks the view protocol marker fails the capability check. On update, the
installer removes the previous PRoot files from `files/bin`. No interpreted game
launcher is packaged; generic Rust
backend code supervises Xvfb, PulseAudio and the guest process.

The display and audio bridges are threads of the session owner
(`backends/android/src/bridge.rs`), not programs in the rootfs. The display bridge
speaks X11 directly: it mirrors the root window into `run/frame.bin` through an
MIT-SHM memfd segment (falling back to GetImage) and injects the app's input lines
with XTest. PulseAudio streams the null sink's monitor through its simple protocol
(`/tmp/pulse-pcm`), and the audio bridge relays that PCM to the app's AudioTrack
socket. Bridge notes and failures go to `run/bridge.log` and end the session with
an error. `cargo test -p banana-android --test bridge` checks both against a real
Xvfb on Linux.

Each title imports from either a game folder or a single `.zar` archive (also
`.zarchive`/`.wua`), picked with the system file chooser. An explicit import
creates a source library entry in private storage: a folder is copied file by
file, and an archive is copied whole (never extracted) into a staging directory
outside the scanned library, checked for a recognized build, and only then
replaces that title's previous folder or archive. Existing private/external game
folders and archives are also found in place and use the common archive provider. Touch button keys come from the current Rust controls bindings.
Input buffering is bounded and resynchronizes held state after reconnecting.

Redistributable legacy libraries can be supplied from per-user dependency storage
with `BANANA_ANDROID_LEGACY32`. They are installed separately from game content.
The runtime installer preserves its previous rootfs/tools during replacement.

## Validation status

The Kotlin frontend compiles against Android API 36. The shared JNI library builds
for aarch64-linux-android with 16 KiB load alignment, `banana-tracer` links for
Android ARM64, and the declared adapter resource package builds. On x86-64 Linux,
the tracer's tests run a shell, coreutils and Python through it against a real
view server. They cover view reads, writes, listing, mappings, rename and
persistence, a view cwd, exec of a view program, and descriptor sharing across
dup and children. Desktop provider, migration and overlay tests exercise the same
View implementation. The tracer's AArch64 code has not yet run on a device.

The previous Android runner had verified Big Buck gameplay on a Galaxy S24 FE;
that result does not certify this new namespace path. All migrated Android profiles
are experimental until the rebuilt APK is tested on real devices. Rootfs/Box64/Wine
runtime inputs and a physical-device run are required for end-to-end APK validation.

### SM-S721U device follow-up (2026-11-07)

The rebuilt APK installs through a streamed update and recognizes all five game
folders. Device testing exposed and repaired the missing packaged artifact catalog,
the then-PRoot cross-build path/tool selection, omitted hard-link emulation, game preload
leaking into support processes, and host paths being passed instead of virtual
guest paths. Xvfb and the frame bridge start after those fixes.

The new namespace path still cannot load Big Buck on this phone. A native probe
under the app UID confirms that `memfd_create` and `ftruncate` succeed, while
`fchmod` and reopening either `/proc/self/fd/<memfd>` or a parent's memfd return
`EACCES`. Android therefore skips cache chmod (guest permissions come from view
metadata), and view protocol 2 replaces procfs reopening with descriptor transfer:
the Rust server sends the file's memfd to the tracer over `SCM_RIGHTS`, and the
tracer passes it into the tracee through a private abstract socket whose peer
credentials it checks. Because a transferred descriptor shares one kernel offset,
the tracer keeps each open description's offset, mode and close-on-exec state
itself (across `dup`, `fcntl`, `fork` and `exec`). `copy_file_range` and `sendfile`
on view files return `EXDEV`/`EOPNOTSUPP` so libc falls back to ordinary reads
and writes. No extracted game fallback was added. Protocol 2 has not yet run on
the device, and gameplay remains unverified for the new session path.

The targeted `banana-ffi`, `banana-compat`, `banana-vfs`, and `banana-android` host
suite passes 41 tests with local socket access enabled. The ARM64 JNI library and
signed/aligned APK build successfully. All five source game folders were restored
after the incremental-install data loss; Jurassic's desktop launcher symlink and
dangling `pmuser` symlink were omitted on Android external storage. Phone-only saves
from the removed installation have not been recovered.
