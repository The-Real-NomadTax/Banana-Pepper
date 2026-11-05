# Minecraft Dungeons Arcade on Linux

The `aspens-crap` branch was pulled and tested against the supplied 3.07 folder on CachyOS x86-64. **Minecraft is not running in-game.** Library loading and snapshot import reconstruction succeed, but protected-asset initialization remains blocked.

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
