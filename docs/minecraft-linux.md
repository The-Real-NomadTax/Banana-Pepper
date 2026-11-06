# Minecraft Dungeons Arcade on Linux

**Current seeded-runner status (2026-11-05):** coin, Start, loadout, level select and the chosen level all run. The game runs in a 60 Hz gamescope window by default; at 60 Hz the player panels, Ready box, score row and gear HUD draw. Still open: the HUD and short banners are intermittent between runs (sometimes missing, or a banner such as "NIGHT IS COMING" stays up), and automated runs have not yet shown a controllable player character after pressing a button in the level. Use the seeded launch below. The later investigation sections are historical records.

## Cabinet type, 60 Hz and level start (2026-11-05)

- **60 Hz:** the cabinet presented vsync'd at 60 Hz and the game's UI timing depends on it. On a 165 Hz monitor the game ran at about 165 FPS (it swaps twice per frame, so `glXSwapBuffers` counted ~330/s) and panels, the Ready box and the HUD were missing. The launcher now runs the sandbox inside `gamescope -r 60` (1920×1080 window). `--no-gamescope` draws straight to the desktop. Keyboard focus accepts the `gamescope` window.
- **Cabinet type:** `SysInfoGameCabinetType` (cached at `0x182a4ac`) is read from dongle offset `0x29` by the skipped HASP block, so it stayed 0. UI scripts branch on it with `BR_ON_PROC`: 1 = four players (`LOOP_4P`, `FADE_4P`, `MINE_4P`), 12 = two, 13 = one. The runner presets 1, matching TeknoParrot's `CabinetType=4 PLAYERS` (`--cabinet-type` overrides).
- **RIO2 boards:** cabinet type 1 also asks the I/O layer for two RIO2 boards (`0x424650`), which stopped boot at "Press TEST to continue". The runner skips only that board-count request (`0x42466c`).
- **Card stock:** `0x441340` reads the cards this cabinet may still dispense from dongle offset `0x32` (cap 7000) into `0x179dee4`. At 0 the game sets its lock flag (`0x179de94`) and every slot shows "GAME LOCKED / ALL DISPENSERS REPORT ERROR". The runner presets 7000 (the save can supply a different count at startup).
- **Dispenser status bits** (`0x41a100`, per player): 0x01 ready (input `0x208`), 0x02 empty (`0x20a`, sets `P%u_CARDEMPTY`), 0x04/0x0C card at exit / held over 500 ms (`0x209`), 0x20 card low (`0x20b`). The adapter keeps reporting 0x01.
- **Transitions:** the load-with-swipe routines (`0x50e440`/`0x50e650`) now keep their native scene unload/load and only replace the final blocking wait for `TRANSITION_OUT_DONE` (which the "transition" scene's scripts never send here) with `SetTrigger("TRANSITION_OUT_DONE")`. The previous whole-routine cut skipped scene loads. A bounded pause there (1 s wait or 60 frames) was tried and rejected: one variant exhausted the 64-byte memory pool, the other crashed at `0x442852` and started level scripts out of order.
- **Level-select freeze:** after a level is chosen and all players are Ready, the native game-select loop (`0x433110`) waits for `GAMESEL_DONE`, which only the transition scene's logo swipe sets. The loop now waits for `LOGO_TRANSITION`, which the level-select script sets at that point. The trigger name is stored in the unreachable HASP block.
- **Testing on a private display:** `ARCADE_GAMESCOPE_BACKEND=headless` plus `--no-host-keyboard` runs unattended; inputs go to `runtime/minecraft-seeded/key-events` (`SDL-scancode 1|0` lines) and screenshots come from the gamescope Xwayland display.

**Measured result:** the full coin → loadout → level select → level flow runs at 60 Hz with player panels, Ready, score row and gear HUD drawn (reference: cabinet photos supplied by the user). Remaining: the intermittent HUD/banners above, and the player character not yet seen after pressing a button in automated runs.

## Virtual card scanners

```sh
cargo build -p arcade-minecraft-io --offline
python3 scripts/minecraft-seeded.py 'games/linux based/Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills][TP]'
```

F1–F4 scan the selected card for P1–P4 while the Minecraft window has focus. The default card is Sun's Grace (`a0000000`); choose another with `--card CODE`. Scanning is useful at the game's card-selection prompt. The game retains its own enabled-card, player-state, duplicate-card and loadout validation.

Click Minecraft to focus it. `5` inserts a P1 credit and `1` sends the P1 primary action/Start; Enter and Space are additional P1 action keys. P1 uses arrows and `Z`/`X`/`C`; P2 uses `IJKL` and `U`/`O`/`P`. `2`–`4` send the other players' primary actions and `6`–`8` their coin switches. Numpad 1–8 are also supported. Focused host evdev events feed a bounded `key-events` protocol because the old SDL/XWayland path can miss keyboard delivery. Once a host key is seen, that source has one input authority, preventing duplicate SDL edges; focus loss releases held keys. The adapter marks the native input packet ready so CoreInputLoop applies the events. Keys are not also forwarded through uinput. Four virtual joysticks still provide device presence. `--no-keyboard` disables keyboard controls and scan hotkeys.

The default render size is cabinet-native 1920×1080. At that size the complete scan heading is visible; a 1280-wide window could show a clipped 1920-wide layout. This alone does not solve missing player panels.

The virtual adapter also reports a healthy ready/taken paper-dispenser sensor at `0x41a100`. The native dispenser task retains its queue, timing, completion callbacks and audit updates. Restored pending jobs therefore complete without physical hardware. A live test cleared all four “Cards Owed / Please take card” overlays, and the runtime save records zero owed for all four players. This represents virtual fulfillment, not physical printing. Original save files are unchanged; only the ignored runtime save is updated by the game.

The Mojang stall was reproduced inside the visual swipe-transition routine: native Start was detected, but the attract task then waited inside `0x50e650`. Skipping the transition routines let the game proceed to loadout and level selection. The seeded runtime first cut between scenes by tail-calling the native trigger setter from all four load/restart variants (`0x50e440`, `0x50e540`, `0x50e650`, `0x50e760`); this is superseded by the tail-only patch described above. `TRANSITION_IN` and `TRANSITION_OUT_DONE` notifications are preserved; the earlier plain-return experiment incorrectly dropped them. `--native-transitions` restores the original routines for diagnosis. This is a visual-transition workaround; the underlying transition failure remains unresolved.

Send any other eight-character, case-sensitive QR code while the game runs:

```sh
python3 scripts/minecraft-cards.py a10g0000 --player 1
```

That example scans Red Snake. The sender requires a running seeded session and does not carry events into the next launch. `--no-cards` keeps the original physical-camera implementation.

The scanners are USB cameras feeding ZBar QR decoding, separate from the executable's PC/SC interface. The x86-64 Rust `arcade-minecraft-io` library replaces the QR worker and camera-ready/preview functions. It writes decoded scans to the game's four native 12-byte result slots under the game's own pthread mutexes. Each virtual scanner has a fixed player assignment, so physical brightness calibration is unnecessary. Ready status is advertised after native card initialization; this preserves startup's physical-camera enumeration. No fake V4L2 devices or copied TP implementation are involved. Camera preview/snapshot diagnostics are not implemented.

The runner checks the complete source SHA256; the adapter checks all patched entry points, mutex ABI entry points, and the scanner-configuration getter before changing memory. Only `game.seeded` receives hooks; child helper processes are excluded. Card events and copied libraries stay in ignored `runtime/minecraft-seeded/`; game originals and captured seed files are unchanged.

Measured validation: six Rust parser/mapping tests, Clippy, runner/import-reconstruction tests, Python syntax checks, and actual desktop captures. GDB confirmed native coin/Start detection and a successful P1 `a0000000` scan (`0`) during loadout. Earlier P1/P2 scans outside an active loadout returned ignored/not-enabled (`1`). The normal launcher also reaches card and level selection without GDB. These checks establish input and scan delivery, not full gameplay or correct rendering of equipped gear.

## Reproduce

From the project root:

```sh
cargo run -p arcade-desktop -- --setup-minecraft-libs games
cargo run -p arcade-desktop -- --probe-minecraft games
```

Named folders such as `games/linux based/Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills][TP]` are discovered automatically. Linux uses the native ELF64 guest inside Bubblewrap; Windows uses WSL2. The separate Windows translation-engine prototype is not used or changed here.

Library setup supports apt/dpkg hosts and Arch/CachyOS hosts with pacman. On Arch it stages `libiw.so.30` from the host package database's HTTPS URL and SHA256 metadata, extracts only that named library, and records provenance in `runtime/minecraft/wireless-source.json`. Other dependencies use existing host libraries. The existing hash-pinned FMOD SDK setup stages the required x86-64 FMOD 1.10 pair locally. No packages are installed system-wide. Libraries, package archives, originals and generated runtime files remain outside Git.

## Measured blocker

The original snapshot SHA256 is `663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b`. The original protected `game` SHA256 is `b880d1f9d27932fdb344dc77d445981ac3000026602260e752124be8a2badbdc`. Both are unchanged.

The normal runtime reconstructs 659 stale imports without changing game instructions. Its loader resolves all dependencies, including FMOD and the locally staged wireless library. A bounded GDB check then reads the native HASP status **7** and wrapper status **4** before the configuration-dependent error logger faults. The runner exits with an explicit failure report rather than claiming process survival or gameplay. Evidence is in `runtime/minecraft/report.json`, `dependencies.log` and `license-probe.log`.

Static inspection confirms that asset initialization passes a 128-byte seed table and a 64-byte seed table to the cabinet decryption API before initializing the file reader. The same encoded pair appears in the supplied GetFile, GetCert and rtinit utilities. Those copies do not supply a decoded alternative. A scan for small response records found game resources rather than an Aliens-style response cache. The user confirmed that only this folder is available.

The branch's earlier isolated bypass experiments already reached archive/localization failures after skipping the cabinet login. Those unsuccessful patches remain excluded from the normal launcher. Skipping that call does not provide the missing asset-decoding state. No Minecraft window, attract screen, controls, audio or gameplay is verified. Further startup progress requires recovering valid decoding state or a usable alternate asset backup; none was found in the supplied files during this investigation.

## Validation

```sh
python3 scripts/test-minecraft-run.py
python3 scripts/test-minecraft-prepare.py 'games/linux based/Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills][TP]/gamedump.exec'
cargo test --workspace
```

The runner tests verify that corrupt downloads preserve an existing library and cannot reach extraction, and that non-HTTPS package metadata cannot initiate a download. Snapshot tests verify that import reconstruction preserves executable instructions and rejects unknown builds.

## Explicit decoder experiment

```sh
python3 scripts/minecraft-asset-probe.py 'games/linux based/Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills][TP]'
```

This separate, 35-second-bounded debugger probe skips the cabinet login/ID block in memory while retaining all five file-reader configuration pointers. It uses a separate runtime/save directory, the desktop display, read-only originals and inert cabinet NVIDIA/XRandR/reboot/shutdown helpers. It verifies the original snapshot hash before using any native addresses. The normal launcher/runtime image is not patched by this experiment.

Measured result: the native reader derives its key and IV from `/pm/g7/minecraft/eFileMaps/fileMap000` and the compiled tables. AES completes, but the decoded file-map text is invalid, so PROGRAM/SCENE archives and localization fail. Cleanup crashes at `0x937c2b`. An independent AES-256-OFB calculation matches all 584 captured output bytes exactly. This rules out a discrepancy between the native AES routine and that independent implementation; it does not establish a working asset key.

The supplied memory snapshot also has zeroed active seed pointers, key-session state and file-map initialization state. It does not retain an initialized decoder session. A bounded local candidate check tried supplied G6 seed windows, Minecraft's secondary application pair and filename conventions against the protected ZIP/version signatures; none of the 1,206 checks recognized a valid output. This is not an exhaustive search of keys or proof that no other recovery is possible.

Probe logs, captured derived values and JSON are under ignored `runtime/minecraft-asset-probe/`. Exit zero means the diagnostic captured a map, not that the game succeeded; `gameplay_verified` remains false and the crash is recorded. The separate secondary-pair/candidate test also failed. The [official software page](https://rawthrills.com/software/) directs users to support/purchase rather than providing a direct software package in that page's content; no alternate package was obtained from it.

### Additional supplied-data recovery checks

The next pass examined the production directory for readable alternate definitions/container signatures and found no candidates. This is a header/text heuristic, not a proof that every byte lacks recoverable information.

The supplied Aliens `hhl_mem.dmp` was checked as a possible source of reusable cipher state. Its byte windows, including reversed windows, provided 429 distinct AES key candidates. Tests against Aliens' known input/output seed pair covered ECB, CBC (including unknown-IV tail comparisons), OFB, CFB and CTR hypotheses, with IVs inferred where applicable. None of the 4,290 known-pair checks matched. This rejects those direct key/mode hypotheses, not every possible encoding of that record.

All 12 sixteen-byte blocks of Minecraft's compiled 128/64-byte seed tables were also checked against the supplied Aliens response records and executable; none matched. No initialized Minecraft key state was recovered. The local audit is `runtime/minecraft/recovery-audit.json`. The normal runtime and originals were not changed during this pass, and Minecraft remains unplayable.

## What the supplied folder is missing

Aliens Armageddon works because its folder includes a `hasp/` directory: 14,674 small dongle response records, each named by the hex input it answers, plus a `teknoparrot.ini`. Those records supply the decrypted asset seeds that the Sentinel/HASP key would normally return. The Minecraft 3.07 folder has neither. Its `minecraft.hvc` is only the Base64 HASP vendor code, which `hasp_login` needs; it holds no key responses. The `Patch` binary is a HASPE Linux envelope loader (`PATCHED_WITH_HASPE_LINUX_LOADER_1.1`), not a response cache.

The local folder was checked against the TeknoParrot Collection RomVault DAT (Eggmansworld/TeknoParrot release 2026-11-02, entry *Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills Linux PC][TP]*, 6,074 entries). Every file is present. `gamedump.exec`, `game`, `Patch`, `minecraft.hvc`, both protected archives and all seven file maps match by SHA1. The only DAT entries missing locally are three empty directories. The same DAT lists 14,674 `hasp/` records for Aliens Armageddon and none for either Minecraft dump (3.07 or 5.00). The official dump therefore contains no dongle responses. TeknoParrot must supply them from inside its own closed-source loader, which this project does not use.

The only routes past the asset blocker are a physical HASP key or response data from outside the supplied dump. Neither is available.

## Seeded launch (2026-11-05): game boots

The two plaintext seed tables were captured from TeknoParrot on Windows with `scripts/minecraft-capture-seeds.ps1` and stored locally as `runtime/minecraft/seeds.json` (not in Git). With them, `fileMap000` decrypts to a valid file map and the PROGRAM/SCENE archives open.

```sh
python3 scripts/minecraft-seeded.py 'games/linux based/Minecraft Dungeons Arcade (3.07)(2021)[Raw Thrills][TP]'
```

The runner builds `runtime/minecraft-seeded/game.seeded` from the import-repaired dump, changing only that runtime copy:

- writes the seeds at `0x16d3d40` (64 bytes) and `0x16d3d80` (128 bytes), and sets the five file-reader pointers that `0x4a4e70` would store
- jumps over the HASP login/decrypt call at `0x430487` (to `0x430542`)
- dongle heartbeat `0x4a4540` treated as success (`0x4a458d`), and session check `0x4a44e0` returns 0
- `NetworkInit` continues when `SysGetCryptoSeed()` can't read the 40-byte dongle memory record (`0x490f79`). That record keys online traffic only.

The original `game` and `Patch` are bound read-only at their cabinet paths so the file-integrity test passes, and the seeded image runs from `/work/game.seeded`. The network stays unshared.

**Measured result:** the game logs `Welcome to Minecraft v03.07` and `Network initialization completed.`, opens a 1280x720 "Minecraft" window and draws the Mojang Studios boot screen (`runtime/minecraft-seeded/boot-screen.png`). The overlays show the next work: *Factory Setup Has Not Been Completed*, P1–P4 joystick not detected, and P1–P4 card scanner not detected. It stayed on that screen from 30 to 90 seconds. Attract mode, controls, audio and gameplay are not verified yet.

`--gdb <script>` runs the same sandbox under GDB for diagnostics, and `--timeout N` stops it after N seconds.

### Joysticks, region and factory setup (2026-11-05)

- **Joysticks:** the game lists joysticks through its built-in SDL, then runs `udevadm info` on each one. A normal Bubblewrap `--ro-bind` of `/dev/input` is `nodev`, so SDL found 0 devices. The launcher now creates four uinput pads (`scripts/minecraft-joysticks.py`) and `--dev-bind`s only those four nodes. The P1–P4 joystick warnings are gone. `--no-joysticks` and `--no-keyboard` turn this off.
- **Region:** the country byte at `0x17bad20` is normally loaded from dongle memory by the skipped init block, so it stayed 0. The launcher presets it (`--country`, default 48, the value in the supplied cabinet's own gamelog), and the setter at `0x4a5440` now stores the byte in memory only.
- **Factory setup:** `ATTRACT_FACTORY_SETUP_INCOMP` is shown while the flag at `0x179d100` is 0. That flag normally comes from dongle memory (`0x438bc0`). The launcher presets it to 1 and makes the loader return success. The warning is gone.

**Measured result:** the game logs `Country Code: 48` and draws the Mojang screen with only the four "Card Scanner is not detected" warnings. It stayed on that screen from 40 to 110 seconds. Attract mode and gameplay are still not reached.
