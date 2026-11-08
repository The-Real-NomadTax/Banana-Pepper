# Android: native Kotlin UI, shared Rust sessions

The Android frontend uses the same folder/ZAR providers, build identification,
compatibility plans, transforms, dirty-block overlays, entry identity and session
API as desktop. Kotlin owns the native library/game UI, touch input, frame display
and AudioTrack output. Rust owns preparation, execution and process cleanup.

Normal, unrooted ARM64 phones are the target. Android apps cannot mount an arbitrary
FUSE filesystem. The Android adapter instead extends the PRoot tracer with a
versioned namespace protocol into `banana-vfs`. No extracted game tree or patched
executable is written to disk.

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

## Build

Use the Linux Android build tools with a JDK, Kotlin 2.2.21, Android SDK and NDK. Cross-building the declared guest adapters also requires LLVM lld-link and i686 GL/GLUT, SDL 1.2 and PulseAudio development libraries (the same dependencies as the desktop Linux adapters):

```sh
apps/android/fetch-proot.sh
# Build/provide rootfs.tar.gz, patched Box64, and optional Wine/x64 libraries.
apps/android/build-apk.sh
```

No phone is needed to build. On an x86-64 Linux host (WSL works), with `qemu-user` and a `proot` binary:

```sh
apps/android/rootfs/make-rootfs-qemu.sh   # Debian arm64 under PRoot + QEMU: build/rootfs.tar.gz, build/box64
apps/android/fetch-wine.sh                 # Windows titles: pinned Wine WoW64 build
apps/android/prepare-x64-libs.sh           # Debian x86-64 libstdc++/libgcc_s for Box64
apps/android/fetch-proot.sh                # Termux talloc/shmem and headers
apps/android/build-proot.sh                # patched PRoot (any NDK: a non-Linux NDK is used through host clang + LLD_DIR)
cargo run --release -p banana-dist --locked -- android-resources
cargo run --release -p banana-runtime --bin banana-session -- --setup-legacy32
```

`build-apk.sh` then runs on Linux, or on Windows in Git Bash with the Windows SDK, NDK and a JDK; it downloads nothing but expects the Kotlin compiler in `build/kotlin-2.2.21`. The APK is signed with the local debug key (`apps/android/debug.keystore`): install it by allowing installs from unknown sources.

For the earlier `com.arcadefoundry` installation, build with
`APP_ID=com.arcadefoundry` and retain its signing key. Back up app data before
updating a development device, and install with
`adb install --no-incremental -r apps/android/build/BananaPepper.apk`.
On the SM-S721U, an incremental update reported success before Android removed
the package and its data; the streamed installation succeeded.

The APK build compiles the ARM64 JNI library, asks `banana-dist android-resources`
for crate-declared title artifacts, and builds pinned PRoot with the namespace
extension. The JNI library retains 16 KiB load alignment. Stock PRoot is rejected
by capability checks. No interpreted game launcher is packaged; generic Rust
backend code supervises Xvfb, frame/audio bridges and the guest process.

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
for aarch64-linux-android with 16 KiB load alignment. The complete patched tracer links for Android ARM64, and the declared adapter resource package builds. Live
Linux PRoot tests exercise virtual executable loading, file/dir operations, copying,
shared/private mappings, persistence and child access. Desktop provider, migration
and overlay tests exercise the same implementation.

The previous Android runner had verified Big Buck gameplay on a Galaxy S24 FE;
that result does not certify this new namespace path. All migrated Android profiles
are experimental until the rebuilt APK is tested on real devices. Rootfs/Box64/Wine
runtime inputs and a physical-device run are required for end-to-end APK validation.

### SM-S721U device follow-up (2026-11-07)

The rebuilt APK installs through a streamed update and recognizes all five game
folders. Device testing exposed and repaired the missing packaged artifact catalog,
PRoot cross-build path/tool selection, omitted hard-link emulation, game preload
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
