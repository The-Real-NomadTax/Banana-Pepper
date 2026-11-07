# Android frontend and runtime

The Android application is written in Kotlin and calls the shared Rust backend
through `libbanana_ffi.so` (JNI). Its library is generated from registered `GameCompat` implementations that declare an Android profile.
Minecraft Dungeons is excluded from Android at the user's request. There is no separate Java game registry or
Android copy of a game's executable patches.

## Current verification

| Title | Android path | Verification |
| --- | --- | --- |
| Big Buck Hunter Pro | box32 + Rust JAMMA/OSS shim | Prior app reached scored gameplay on SM-S721U. The Kotlin/Rust launch path renders calibration, receives gun/Test/Start events, and advances to right-gun calibration. |
| Jurassic Park 2015 | box32 + import repair, plaintext script checks, shader overrides | Attract video, mission selection and a 3D mission sequence render after fixing denied real-time file-worker threads. Coin/Start and gun events reach the game. Full gameplay and sound regression testing remain. |
| Aliens Armageddon | box32 + existing Rust executable preparer and legacy PhysX libraries | Experimental. Worker scheduling is repaired, but static Xlib code encounters a Display layout mismatch during initialization. Gameplay remains unverified. |
| Super Cars | box64 + x86-64 Wine WoW64 + cabinet DLL | Restoring localhost resolution gets past its hostname crash to a link-system error. The app now declares the network permission required for its sockets; retesting remains. |
| Ford Racing | box64 + x86-64 Wine WoW64 + cabinet DLL | Renders its loading screen and receives Start. The virtual audio device avoids its previous sound-loader fault; racing remains unverified. |

Listing a title or enabling Play establishes that its known files and declared
runtime components are present. It does not establish Android gameplay support.
Only Big Buck carries the historical Android verification badge. All other
profiles explicitly say experimental.

All five supplied game folders are present on the phone. Wine startup requires
native X11 extension libraries and fontconfig/fonts, now included in the rootfs.
Windows titles use a private PulseAudio null sink and a PCM transport to Android.
These observations do not establish that every listed title is playable.

## Using the app

1. Install the APK. Its first launch installs proot, the Linux rootfs and tools.
   Updating tools preserves the rootfs, imported games and existing saves.
2. Choose **Import a game folder**, select a title and choose your own folder.
   Imports are staged and identified by the Rust backend before replacing an
   earlier import. A failed or unrecognized import retains the old files.
   The folder may be the title itself or a conventional library containing it.
3. Games in `Android/data/<application ID>/files/games` are discovered in place.
   **Ready to play** filters to titles whose files and required tools are present.
   **Details** lists missing runtime components.
4. **Play** opens a landscape game view. Gun games use touch to aim/fire and a
   second finger or **RELOAD** for right-click. Cabinet and racing buttons come
   from each title's Rust controls. Controls are scrollable at either side.
5. **EXIT** or Back confirms session closure. Held inputs are released on pause
   and cancellation. **Session logs** displays the last launch failure.

Sources and prepared executables are separate. Big Buck migrates previous
`files/saves/bbh3user` state into its session before seeding any missing files.
Writable sessions are under `files/sessions/<game-id>`; the last session's
framebuffer, command plan and logs are under `files/run`.

## Build

Requires JDK 17, Kotlin 2.2.x (compatible with the SDK D8), Android SDK build-tools,
platform 36+, NDK r27+, Rust targets `aarch64-linux-android`,
`i686-unknown-linux-gnu`, `i686-pc-windows-msvc`, desktop multilib graphics
libraries and `lld-link`. The build can use `KOTLINC=/absolute/path/to/kotlinc`.
If the local `build/kotlin-2.2.21/kotlinc` installation exists it is selected
automatically. No Gradle or Java app sources are required.

```sh
apps/android/fetch-proot.sh
apps/android/rootfs/setup-dev-device.sh
apps/android/rootfs/make-rootfs.sh
apps/android/fetch-wine.sh
apps/android/build-apk.sh
adb install -r apps/android/build/BananaPepper.apk
```

`fetch-wine.sh` pins Wine 11.0 amd64 WoW64 and verifies the publisher's SHA-256.
The APK includes it when `build/wine` exists; otherwise the racing profiles show
that runtime as missing. The publisher's release is
<https://github.com/Kron4ek/Wine-Builds/releases/tag/11.0>.

The build packages locally prepared Jurassic libraries from
`runtime/jurassic/libs`. `prepare-x64-libs.sh` collects GCC runtime libraries
for Wine. `make-rootfs.sh` also exports the patched box64 into `build/box64`,
which is bundled ahead of the rootfs copy. Games themselves are never bundled.

The default application ID is `com.bananapepper`. To update the earlier installed
app without deleting its private data or changing its external game directory:

```sh
APP_ID=com.arcadefoundry apps/android/build-apk.sh
adb install -r apps/android/build/BananaPepper.apk
```

The same local signing key must be retained. Both IDs use the Kotlin namespace
`com.bananapepper`; manifest activities are fully qualified. `RELEASE=1` disables
debugging. The debug APK supports `adb shell run-as <application ID>`.

## Runtime boundaries

- Rust supplies title identity, executable fingerprints, preparation, touch
  actions, runtime requirements and the proot command plan. Game-specific
  Android code lives in each game's `android.rs`, behind the shared API.
- Kotlin supplies Android UI, folder-picker copying, transactional runtime
  installation, process lifecycle, frame presentation, input sockets and
  AudioTrack. The generic `rootfs/run-game.sh` supervises Xvfb, afbridge and the
  command supplied by Rust. No cabinet boot scripts are invoked.
- ARM64 Android uses box64 with BOX32 enabled; AArch32 Android support is not
  required. The existing sectionless-ELF patch remains necessary.
- Xvfb/llvmpipe supplies a 640×480 software display. Native JNI libraries and
  APK entries use 16 KiB alignment; this does not establish that box32/the Linux
  rootfs works on a phone actually booted with 16 KiB pages.
- Big Buck's OSS audio streams PCM into AudioTrack. SDL and Wine audio
  have not been validated through Android's audio bridge.
- proot supplies a compatibility filesystem view, not a security sandbox.
  target SDK 28 is deliberate because the runtime executes binaries in private
  app storage. Abstract input/audio sockets use a random per-session token;
  Xvfb's TCP and abstract local transports are disabled.

## Checks

```sh
cargo test -p banana-ffi -p banana-compat --offline
cargo test -p banana-linux-shim --target i686-unknown-linux-gnu --offline
```
