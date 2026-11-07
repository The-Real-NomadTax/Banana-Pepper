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

The APK build compiles the ARM64 JNI library, asks `banana-dist android-resources`
for crate-declared title artifacts, and builds pinned PRoot with the namespace
extension. The JNI library retains 16 KiB load alignment. Stock PRoot is rejected
by capability checks. No interpreted game launcher is packaged; generic Rust
backend code supervises Xvfb, frame/audio bridges and the guest process.

The existing folder import UI is preserved. An explicit import creates a source
library entry in private storage; existing private/external game folders are also
found in place. ZAR sources placed in those library directories use the common
archive provider. Touch button keys come from the current Rust controls bindings.
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
