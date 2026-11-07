# Minecraft Dungeons Arcade on Linux

The native plan recognizes the supplied gamedump, reconstructs imports in memory, and exposes the patched executable through the shared FUSE view. Captured cabinet-response records are required for the seeded launch. Their contents are user supplied.

Prepare declared dependencies through the shared operation:

```text
arcade-desktop --action minecraft setup SOURCE
arcade-desktop --entries
```

Setup downloads libraries into per-user dependency storage without installing system packages. The current recipe uses apt/dpkg plus the fingerprinted FMOD archive; its licence is retained with those local libraries. Dependencies and captured records are excluded from application distributions.

Place seeds.json in the entry's state directory, then start with `--action minecraft launch SOURCE`. Legacy captured records are imported before planning. A missing or incompatible record is an explicit error.

The title adapter supplies virtual scanners, dispenser hooks and keyboard mapping. Native supplies uinput controller presence and bounded, focus-aware input frames. Access to /dev/uinput and the selected input devices is required. Held keys are released when focus or transport is lost.

During a running session, `--session-action ENTRY scan-card JSON` accepts an object with player 1–4 and an eight-character alphanumeric code. F1–F4 use the configured default card through the same structured transport. There is no separate card utility or growing event file.

Original content stays read-only. Game writes persist in the entry overlay. Support remains experimental until gameplay, graphics, audio and saves are verified with this mounted pipeline.
