> Historical verification record. References below to Python/shell launchers
> describe earlier implementations. Registered game actions now use Rust in
> `game-compat/` with compiled native/Wine/WSL helpers; those launch scripts
> have been retired. Historical gameplay observations do not verify the new
> orchestration implementation. See `docs/workspace.md` for the current design.

# Initial build verification

Verified on the current Linux x86-64 host:

- `cargo check --workspace --offline` passed.
- `cargo test --workspace --offline` passed; the subsequent sectionless ELF test also passed with `cargo test -p arcade-core --offline` (four core tests total).
- `cargo clippy --workspace --all-targets --offline -- -D warnings` passed.
- `cargo run -p arcade-desktop --offline -- --scan games` read all four supplied binaries. Local JSON is in `reports/local-scan.json` (excluded from source control).
- ELF/PE format and 32-bit i386 identification agreed with `file`; all three ELF entry addresses agreed with `readelf -h`.
- Desktop window rendered successfully on X11; loaded library, binary diagnostics, runtime status and input monitor were visually inspected. Wayland was unavailable in this execution environment. Sandbox display access required running the GUI outside the sandbox.

Observed import counts: Aliens 13, Jurassic Park 16, Big Buck Hunter 7, Super Cars 13. These are statically declared libraries, not resolved dependencies.

No games were launched. Gameplay, cabinet communication, runtime dependencies, Windows, macOS and Android support have not been verified.

## Big Buck Hunter startup backend

A subsequent build implements a private-display startup test. The Rust CLI backend was run end to end and detected the game window. The captured frame showed Left Gun Calibration. The game remained running until the deliberate 8-second timeout (124). Its source SHA256 remained unchanged. Runtime SHA256 was `60d45e2ef1e758d9fcba3732a466135e6b6d22eac7a7389c085e239c5e46e87b`.

Six core tests passed, including rejection of unknown executable builds and verification that reconstruction changes only the intended byte ranges. Workspace tests and Clippy with warnings denied passed. The guest shim built for i686 Linux. The remaining blocker is the serial JAMMA interface; gameplay is not validated. See `big-buck-startup.md`.

## Virtual JAMMA input progression

The guest-side shim now installs a build-specific JAMMA API adapter before game initialization. Private-display tests demonstrated TEST leaving calibration, START reaching attract/game selection, and mouse shots advancing through mode selection, Whitetail Adventure, hunting instructions and Player 1 site selection. Checkpoint PNGs are in `runtime/big-buck/`. The test sends fixed scripted inputs; the egui preview remains a still frame.

The serial reset loop no longer runs through the virtual adapter. Unknown JAMMA operations return `-9000`; several setup/output operations are explicit no-ops, so hardware compatibility is incomplete. `/dev/dsp` audio remains unavailable and the game logs sound-player failures.

Workspace tests passed (six core tests plus the switch-transition test). Native i686 tests passed for switch transitions and the assembly variadic bridge's pointer outputs, dirty acknowledgement and unknown-operation rejection. Both workspace and i686 Clippy checks passed with warnings denied. The original executable SHA256 was rechecked and remained unchanged.

The final progression run selected a Whitetail site and captured a rendered scene with running deer in `runtime/big-buck/hunt.png`. This verifies scene rendering after menu navigation; scored shooting and pump/reload behavior remain untested.

## Interactive desktop launch

Added `--launch-big-buck` and the workbench's **Play on my display** button. The game was launched on the user's current X11 display following their explicit request. A visible 640×480 game window was confirmed and its game-mode selection screen inspected. The process was left running for the user's testing. Workspace tests and Clippy passed; the i686 shim build and Clippy also passed. Desktop mode uses separate runtime saves, no scripted inputs and no timeout; Esc exits. Audio remains unfinished.

## OSS audio bridge

The game now initializes its audio mixer and streams S16LE stereo at 24000 Hz through the Rust OSS compatibility layer to PipeWire/PulseAudio. A private test-sink capture contained non-silent audio (peak 16289, RMS 1632.06 over 1,277,952 frames). The current run had no sound-load/no-free-player failures and no unsupported audio ioctls. The test sink was removed after capture. A normal desktop relaunch produced an active, unmuted `Banana pepper` / `Big Buck Hunter Pro` playback stream with the expected format. No microphone or normal desktop mix was captured.

All three native i686 tests passed, including OSS negotiation and the C ABI/file-I/O checks. Workspace tests and Clippy passed; i686 Clippy passed with warnings denied. Audible quality and latency still need user testing.

## Jurassic Park native investigation

Staged hash-pinned NVIDIA PhysX 2.8.1 and 32-bit Theora libraries locally, without installing system packages. The original Jurassic executable now resolves libraries and exits at refused localhost:1947 licensing-service connections. The supplied modified image SIGSEGVs during initialization. Native repair mode clears nine invalid undefined-symbol values, enables eager binding and rebinds two identified startup imports; it advances farther, but still crashes through another saved pointer. No game window or gameplay is claimed.

All three Rust diagnostic modes were exercised and saved structured results/traces. Both original executable checksums were recorded and preserved. Seven core tests plus the switch-transition test passed; workspace and i686 Clippy checks passed with warnings denied. See `jurassic-startup.md` for exact commands and remaining work.

## Jurassic import reconstruction

Recovered 218 libc and 54 pthread imports using the exact matching Ubuntu i386 2.19-0ubuntu6.6 reference; five additional PhysX/C++ imports are explicitly identified. Matching debug build IDs recover optimized libc implementations. A normal Bubblewrap diagnostic and a GDB run both reach `Game dongle initialization failed!` at `src/game_sys.c:760` and exit normally rather than hitting the previous stale-pointer crashes. No display is attached and no window is claimed. The workbench now selects the repaired diagnostic. Native reports distinguish the game assertion from generic service refusal and never treat exit 0 as gameplay success.

Workspace tests include assertion/service/crash classification. Workspace and i686 Clippy pass with warnings denied. Import generation reproduces the pthread table byte-for-byte; wrong reference versions and mismatched debug build IDs are rejected without overwriting an existing output. Original executable checksums are preserved. See `jurassic-startup.md` for recovery details.

## Jurassic graphics/resource exploration

Expanded recovery with 37 libm, 42 SDL and 123 core OpenGL bindings: 474 generated entries plus thirteen supplemental bindings/adapters. Wrong adjacent SDL builds and modern NVIDIA dispatch layouts fail independent startup/GLSL fingerprints without overwriting output. Vendor extension bindings were excluded from generic offset recovery and identified from their callers. VSync delegates to SDL swap control to avoid premature vendor GLX requests.

The explicitly separate debugger experiment skips dongle initialization, validates all 1,516 plaintext script backups, mounts them read-only, and recognizes their plaintext marker before the decryption branch. It creates a Jurassic Park Arcade window and reaches SDL_GL_SwapBuffers, but all frames through 60 seconds remain black. Shader108 variants and JPS cabinet audio report failures. This does not establish gameplay, dongle support or audible output. Source game files remain unchanged. The reproducible entry point is scripts/jurassic-explore.py.

Follow-up: restored three supplemental GLX imports used by the movie extension loader (thirteen supplemental entries total). Mesa shader logs identified mixed legacy/named fragment outputs; six runtime-only shader overrides eliminate those compile failures. The corrected `/pmuser` alias allows adjustment/audit saves. Post-fix short probes reach GLCGZ movie decoding and rendering with no SIGSEGV, but direct/root captures stay black. Mesa reports draw-buffer/texture-attachment errors which still require diagnosis. All three trusted i686 shim tests pass, including input/audio ABI regression checks. The explicit experiment is reproducible with `python3 scripts/jurassic-explore.py --seconds 10`; its result never claims gameplay or dongle support.

## Jurassic desktop startup achieved

The host RTX 3050 renders a trusted 32-bit SDL color test with the same sandbox/shim. Game framebuffer readback also contains nonblack image data. Disabling SDL12COMPAT_OPENGL_SCALING resolves the actual black screen: desktop captures show the Jurassic attract video. The extra experimental FBO blit and direct-render overrides were discarded. Forced windowed SDL video prevents desktop resolution changes; cabinet nvidia-settings commands are privately disabled.

A Rust-prepared desktop snapshot now boots without GDB. The exact hash is 9fd96c7e1d14c8863c1e790545c21e9d81f6f72a6b8cb16686f25da0fac313bc. It omits cabinet security initialization and consumes all validated plaintext script backups. `--launch-jurassic` and the workbench button run indefinitely on the user's display. Native captures show rendered attract frames; Escape closes the game and the launcher exits successfully. Main Jurassic audio remains dummy, and gun/coin/start input, scored gameplay and gameplay PhysX behavior are not established.

Workspace tests (including atomic rejection of mismatched desktop patch sites), workspace Clippy and i686 Clippy pass. Original executable hashes are unchanged.

## Jurassic desktop controls

The Rust i686 shim now installs desktop maps through the game's own input mapping layer. Source SDL press/release/held processing is retained. Two failures were reproduced and corrected: late cabinet map installation overwrote mouse coordinates, and physical-gun smoothing blended mouse aim with unavailable cabinet samples. Desktop maps now stay last after map rebuilds, and physical-gun smoothing is disabled only for the desktop profile. Fingerprint checks validate the pointer/mapping ABI before enabling it; original executable files are unchanged.

Verified on the desktop: 1 opens mission selection; a visible mouse reticle moves; the user confirmed aiming and selection work; held left fire produces muzzle flashes and player-one score 75 in an actual level; T opens the cabinet diagnostic menu. A full playthrough and player-two aiming are not verified. Right-click is mapped to the secondary trigger but its gameplay action is not established. Free-play settings prevent credit-count verification of coin keys. The RIO disconnected warning remains; no complete RIO protocol/hardware emulation is claimed. Jurassic audio remains disabled.

Workspace tests and workspace/i686 Clippy pass. All five trusted i686 shim tests pass, including regression coverage for desktop-map priority after native cabinet-map installation/rebuilds, duplicate prevention and atomic rejection of invalid/full map tables. Temporary input tracing and the diagnostic InputGet hook were removed from the final code.

## Jurassic RIO banner suppressed for desktop mode

The banner renderer's nine fingerprint-checked error-flag reads now use a display-only copy with the RIO-disconnected bit masked. The actual cabinet flags, reconnect behavior, disconnect audits and logging remain unchanged; other error bits remain available to the banner renderer. The first attempted change targeted a log call and did not hide the banner; that change was removed. No game asset or executable file is edited.

Desktop visual verification shows active gameplay with score 525 and no red RIO banner. The updated session remains open for the user. The i686 shim builds and passes Clippy with warnings denied, and the desktop workbench rebuilds successfully. Original game_original SHA256 remains 6bb8a70dc0ee3f9d7ac3672d7dc8cf4aadec44b0f96c050ace96a7633d26af11.

## Aliens Armageddon first native probe

Added a hash-verified Rust snapshot preparer, experimental desktop CLI/workbench diagnostic, isolated Bubblewrap harness and Rust csv_write2 ABI implementation. Original SHA256 is 297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db; prepared SHA256 is 1f7fe9b70005bb5669d474cf759a1d1f2a3242ce03075bbd9bfb3a3e6b788631. Only unused ESD/absolute CSV dependency declarations and the fingerprint-checked fork/ptrace supervisor are changed in the separate snapshot.

The first tracing attempts triggered the cabinet supervisor's error handler before actual initialization, misleadingly failing audits with uninitialized metadata. Omitting that supervisor exposes the real path: dongle initialization fails, the version banner is logged, six G6 RTDT headers fail to load, and a null configuration access crashes at 0x080def91. A debugger check on that real path observes invalid compressed data after the native decoder (zlib incorrect-header error). Readable counterpart assets exist for nine directories but matching plaintext G6 headers/shaders were not found; plaintext audit parsing requires missing AudDef metadata. The final normal probe captured a black 1360×768 window. Rendered Aliens content, controls, audio, gameplay and dongle support are unverified. See docs/aliens-startup.md.

Final clean Aliens probe records exit 139, dongle_initialization_failed=true, rtdt_definitions_failed=true, segmentation_fault=true and window_captured=true; the captured 1360×768 window is entirely black. Workspace tests pass (9 core, 2 desktop, 2 host shim), workspace/i686 Clippy pass, and all six trusted i686 shim tests pass. The original executable hash is unchanged. No Aliens process was left running after the failed probe.

## Aliens playable desktop runtime

The earlier asset-decoder failure is resolved. Two supplied response records contain the decoded 512-byte and 256-byte asset seeds. Exact hash/length validation precedes restoring them in a separate Rust-prepared runtime image. The native filename mixer/AES/zlib pipeline now loads all six production type-definition headers and the production game assets without plaintext overlays. Native dongle initialization still logs failure; no complete dongle implementation is claimed.

Windowed 1360×768 presentation and the Aliens-only GLX SGI-to-EXT swap-control adapter resolve the black presentation/vendor BadValue problems. Native desktop input mode removes the physical-gun Start restriction and supplies mouse maps; keyboard 1/5 map Start/coin, with Start also confirming menus. All runtime executable changes use fingerprint guards. The workbench has Open Aliens on my display, and --launch-aliens runs without GDB or a timeout.

Verified mode selection and rendered 3D gameplay, with health/ammunition HUD, enemies and kill totals of 20 and 34 in captures. The user confirmed: "yeah ive been playing everything works and looks good". Manual confirmation is recorded in runtime/aliens/gameplay-verification.json and a capture in gameplay-verified.png; automatic startup result classification alone does not certify gameplay. The game remains open on the user's display. Copied saves currently use Spanish. Audio is disabled; a full playthrough, player two and all advanced weapon/control behavior remain unverified.

Prepared SHA256: 9bd2332bb9c49adb5a3f0854c8c52532f0c3ab29877cee2dae9eb7c8371c5d61. Original Aliens and Jurassic hashes were rechecked and remain unchanged. Workspace tests (9 core, 2 desktop, 2 host shim), all six native i686 shim tests, workspace Clippy and i686 Clippy passed. The desktop launcher rebuilds and formatting/Python syntax checks pass. Existing Jurassic/Big Buck adapters were not changed.

## Desktop audio

Desktop launches of Aliens and Jurassic Park now route SDL audio to the user's PulseAudio/PipeWire session through the shared `scripts/arcade_audio.py` helper. Only the playback socket (and Pulse cookie, if present) is exposed inside Bubblewrap. Aliens' statically linked SDL 1.2 uses its built-in `pulse` driver (which loads the host 32-bit `libpulse-simple.so.0`); Jurassic's host sdl12-compat uses `pulseaudio`. Diagnostic probes stay on the silent `dummy` driver. Set `ARCADE_AUDIO=0` to force silence or `ARCADE_AUDIO_SINK=<sink>` to pick an output. If no Pulse socket is found the launcher falls back to `dummy` and says so. `audio_driver` in each `result.json` records the choice. Jurassic's separate cabinet/rumble (JPS) audio is still not implemented. Audible output has not yet been confirmed on the user's desktop.

## Customizable controls and Aliens test button

Added `arcade-core::controls` (`controls.cfg`), with defaults matching the earlier fixed keys plus new Aliens bindings. The workbench Controls panel captures keys, swaps conflicts, resets defaults and saves atomically. Aliens' preparer rewrites all sixteen fingerprint-checked switch operands (Test/Service/Volume/P2 Start newly mapped) and records the prepared hash for the harness. The Jurassic shim builds its button maps from `ARCADE_JURASSIC_KEYS`; mouse aim/pointer maps stay fixed and last. The Big Buck shim reads `ARCADE_BBH_KEYS`; private-display probes keep the scripted default keys.

Verified off-device only, because the user's computer shell was unavailable: controls tests (parse/round trip, normalization, rejection, rebind/swap), Jurassic map tests and Big Buck key parsing all pass, and Clippy with warnings denied is clean on these modules. `aliens.rs` type-checks against minimal goblin/sha2 stand-ins. Every Aliens patch site was confirmed in the supplied executable. **Not yet verified:** a full `cargo build`/`cargo test`/Clippy of the workspace (including the egui panel) and the i686 shim, and Aliens Test/Service/Volume behavior in game.

## Aliens quit handling

Aliens previously had to be killed from a terminal. The new shim watcher (`aliens_exit.rs`) adds Esc-to-quit and a working close button; see `aliens-startup.md`. An x86-64 integration test ran it against Xvfb with SDL-shaped windows and XTest. It found only the window with the matching class, removed WM_PROTOCOLS (again after it was re-set), exited on Esc only with the game focused, and rescanned after the client exited. Clippy is clean on the module. **Not yet verified:** the i686 build and the behavior with the real game and window manager.

# Windows / Minecraft verification (2026-11-04)

- Native host: x86_64-pc-windows-msvc, Rust 1.99.1.
- `cargo build --workspace`: passed. Existing Jurassic dead-code and filesystem
  incremental-hardlink warnings remain.
- `cargo test --workspace`: 12 tests passed. Linux shim tests are excluded on
  Windows; they still require a Linux build to validate the Linux guest code.
- Actual game directory with spaces, parentheses and square brackets: discovered
  and inspected successfully as sectionless ELF64; verified SHA256 recorded in
  `docs/minecraft-windows.md`.
- Windows executable -> WSL2 Ubuntu -> Bubblewrap dependency probe: executed.
  Local Ubuntu libraries resolve GLU, OpenGL, LAPACK, BLAS, Fortran, wireless and
  PCSC. Remaining unresolved libraries are `libfmod.so.10` and
  `libfmodstudio.so.10`. See `runtime/minecraft/report.json` for the local result.
- Minecraft gameplay is blocked and has not been verified. No claim of working
  keyboard/controller input, security hardware or card readers is made.
- Visual UI verification was unavailable because the computer-use helper failed
  initialization on the workspace path containing square brackets.

## Subsequent Minecraft investigation (2026-11-04)

- Retrieved the archived FMOD 1.10.20 Linux SDK, verified its pinned SHA256 and
  extracted only the x86-64 runtime pair and license. Local provenance is in
  `runtime/minecraft/fmod-source.json`. The setup command reproduces this from
  the cache; these binaries are excluded from source control.
- Actual Linux dependency resolution now succeeds with no missing libraries.
- GDB identified stale `__libc_start_main`/other GOT addresses in the supplied
  memory snapshot. Rebuilt 659 named imports as ELF64 relocations in a new data
  LOAD. The normal preparation changes ELF metadata only, preserving all game
  instruction bytes and the source snapshot. Both local preparation tests pass.
- Normal reconstructed startup reaches cabinet security initialization and
  fails with SIGFPE in the configuration-dependent error logger.
- A temporary investigative startup patch reached Mesa llvmpipe OpenGL threads,
  but failed PROGRAM/SCENE archives and localization, then crashed in cleanup.
  That failed patch is not included in normal preparation. The trace is saved
  locally as `runtime/minecraft/gdb-desktop-asset-failure.log`.
- Protected ZIP/config/version data still lacks a working decoder. Diagnostics
  now report `protected_asset_decoder_required` with an empty missing-library
  list instead of repeatedly launching the observed crash.
- Windows workspace tests: 12 passed. Additional snapshot preparation tests: 2
  passed against the supplied local file. Windows desktop rebuild passed.
- Minecraft remains unplayable; no attract screen or gameplay is claimed.


## Final Minecraft startup result

- Bounded GDB capture reads actual HASP status 7 (key not found), with wrapper
  status 4, before the error logger faults. The current probe reports
  `hasp_key_not_found`; the earlier protected-asset classification is superseded.
- A separate investigative copy skips the login block while preserving both
  asset-seed pointers. It still fails PROGRAM/SCENE archives and localization,
  then crashes in cleanup. The ordinary runtime image is unchanged by this test.
- The same encrypted seed tables occur in the bundled GetFile, GetCert and
  rtinit utilities. No usable decrypted seed state has been recovered.
- Minecraft has not reached an attract screen or gameplay. WSL remains the
  Windows backend; the separate native engine remains paused.
- Final Windows workspace test run: 20 passed, including eight native-runtime
  tests. Snapshot preparation tests: two passed. Existing incremental-cache
  hard-link warnings remain.

## Minecraft Linux host follow-up

Pulled origin/aspens-crap at 35fa190 and reproduced startup on CachyOS x86-64 using the supplied Minecraft 3.07 directory. Existing uncommitted Android work was preserved in a named stash on android-big-buck before switching branches. No Android changes were merged into the Aspen branch.

Added Arch/CachyOS library setup: pacman's package metadata supplies the wireless_tools HTTPS URL and SHA256; only libiw.so.30 is extracted into the local runtime cache. FMOD setup uses the existing pinned SDK archive. Both setup and loader checks succeed, with no system package installation and no missing game libraries.

The bounded native diagnostic again reads HASP status 7 and wrapper status 4. Static inspection locates the 128/64-byte seed initialization and confirms that GetFile/GetCert/rtinit contain the same encoded seed pair. No decoded response cache was found; the user confirmed only the supplied folder is available. No new security/asset bypass patch is applied. Minecraft remains blocked before a game window or gameplay; see minecraft-linux.md.

All 17 host Rust workspace tests pass. Both snapshot reconstruction tests and both new package-download rejection tests pass. Four existing dead-code warnings occur in the Windows-oriented native-runtime test helpers on Linux. Python compilation and git diff whitespace checks pass. Original game/dump hashes remain unchanged. Libraries and diagnostic logs stay under ignored runtime/minecraft; the changes to source/docs are local on aspens-crap.

## Minecraft decoder experiment beyond cabinet login

Added scripts/minecraft-asset-probe.py as an explicitly separate diagnostic. It hash-checks the supplied snapshot, prepares its own runtime/save directory, skips login/ID in GDB while retaining the file-reader configuration pointers, and captures the native first-map key/IV/output. Cabinet shell display helpers are inert and the test is bounded to 35 seconds. The normal launcher and original executable are unchanged.

The derived filename is /pm/g7/minecraft/eFileMaps/fileMap000. Native AES returns zero but its output is invalid map text. Independent AES-256-OFB agrees byte-for-byte over the full 584-byte map. PROGRAM/SCENE and localization still fail; cleanup SIGSEGVs at 0x937c2b. A bounded 1,206-check supplied-candidate/filename test found no recognized ZIP/version plaintext. The source dump's active seed pointers, licensing context and map-init state are zero. No usable initialized decoder state was recovered, and no working-game claim is made. Runtime evidence is excluded from Git.
