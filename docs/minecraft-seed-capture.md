# Minecraft seed capture on Windows (handoff)

Goal: record the two decrypted asset seed tables that TeknoParrot's dongle emulation produces for Minecraft Dungeons Arcade 3.07, so Arcade Foundry's own Linux runner can use them without TeknoParrot.

## Why

- The supplied folder matches the official TeknoParrot 3.07 dump file-for-file, and that dump has no `hasp/` response cache (see `docs/minecraft-linux.md`).
- `TeknoParrot64.dll` is packed, so the answers can't be read from the file.
- On Linux the TeknoParrot loader spins without loading the game, so the capture has to happen on Windows.

## What the game does (gamedump.exec, SHA256 `663b1479…6be3b`)

`0x430487` calls `0x4a4e70(rdi=0x16d4220, rsi=0x16d41e0, rdx=0x16d3e00, rcx=0x16d3d80, r8=0x16d3d40, r9=0x1175a4c)`.
`0x4a4e70` calls the HASP decrypt wrapper `0x64da60(ctx=0x17bad40, buf, len)` **in place** on:

| VA | Length | Encrypted starts with |
| --- | --- | --- |
| `0x16d3d80` | 128 | `7937348ba3964ea3…` |
| `0x16d3d40` | 64 | `73b25110043c7fbf…` |

After a successful startup these addresses hold the plaintext seeds. `0x16d41e0` is never decrypted, so the script uses it to find the guest base if ElfLdr2 doesn't map the ELF at its native address.

## Steps on Windows

1. Get `scripts/minecraft-capture-seeds.ps1` onto the Windows machine.
2. Open PowerShell as the same user that runs TeknoParrot:
   ```powershell
   powershell -ExecutionPolicy Bypass -File minecraft-capture-seeds.ps1
   ```
3. Launch **Minecraft Dungeons Arcade** from TeknoParrot (subscription key registered) and wait for the attract screen.
4. The script writes `minecraft-seeds.json` (192 bytes of seeds as hex) and exits. Bring that file back to the Linux machine as `runtime/minecraft/seeds.json`.

If it times out with `base found: False`, the game was never loaded into memory. If it times out with `base found: True`, the game loaded but the tables were never decrypted, so TeknoParrot may be handling the decrypt somewhere else. Report either result.

## Next step on Linux

The runner writes the captured plaintext into the runtime copy at `0x16d3d40`/`0x16d3d80`, skips the cabinet login/decrypt block, and launches. Earlier experiments already showed that skipping only that block preserves the file-reader seed pointers.
