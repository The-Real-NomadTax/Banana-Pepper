# Skylanders Cloud Patrol

The 2014 touch-screen cabinet build: a 32-bit Windows Unity 4.2.2 player
(`Skylanders.exe`, SHA256
`c9fa7f38ab8cc58ba3e2daaa806d07b87e730a9d49e505f0a1881882b99fc2e9`). It runs
under Wine on Linux. Crate: `game-compat/skylanders-cloud-patrol`.

## Files

Put the cabinet's `DataPC` folder in the games library as
`games/skylanders_cloud_patrol/` (the folder holding `Skylanders.exe` and
`Skylanders_Data/`). `Skylanders Cloud Patrol/`, `Skylanders/`, `DataPC/` and
the game folder itself are found too, as is a ZArchive of any of these.

## Build and run

```sh
cargo build -p banana-skylanders-touch --target i686-pc-windows-msvc --release
cargo run -p banana-desktop -- --launch-skylanders games
```

`just build` builds the adapter with everything else. Wine must be on `PATH`
(or set `ARCADE_WINE`). In the workbench, select **Skylanders Cloud Patrol →
Play game**.

## What the launcher does

Each launch stages `runtime/skylanders/game`: the executable is copied and the
1.2 GB of Unity assets are symlinked, so nothing in the source folder is
written. The cabinet-only files are left out: the Java updater
(`Skylanders.jar`), its SSH tunnel (`plink.exe`, `sshlink.ppk`), the launcher,
the boot script (which reboots the PC when anything fails) and a disk-cache
registry tweak.

Two cabinet clients are replaced by one adapter DLL
(`game-compat/skylanders-cloud-patrol/hook`), staged under both names:

- **`PQMTClient.dll`, the PQ Labs touch screen.** The original connects to a
  touch server on 127.0.0.1:21303; without one the game never learns the
  screen size and throws `DivideByZeroException` every frame. The adapter
  reports the desktop mode as the touch screen (the game scales touches by
  `Screen.currentResolution` with integer division) and sends one contact,
  id 0, in window pixels. The game ignores contact ids at or above its finger
  count (6).
- **`KaboomComDll.dll`, the Kaboom coin/ticket board** (serial). The game polls
  the coin counter and credits one coin each time it goes up, then resets it.
  The adapter raises the counter by one per poll while coins are queued, so
  quick coins are not merged. Ticket payouts complete at once.

The Matrix USB dongle is not emulated; the game runs without it. Unity is
started with `-screen-width`, `-screen-height` and `-screen-fullscreen` from
`settings.cfg`. The Wine prefix, saves and logs live in `runtime/skylanders/`:
`game/Skylanders_Data/output_log.txt` (Unity),
`game/arcade-skylanders.log` (touch) and `game/arcade-skylanders-board.log`
(coins).

## Controls

| Action | Default |
| --- | --- |
| Touch / shoot at the cursor | Left mouse button (always), Space, controller A or RT |
| Insert coin | 5, controller Back |
| Move the cursor | Either controller stick |
| Exit | Escape, or Back + Start |

A game costs 4 coins with the cabinet's default operator settings. Bindings
are in `controls.cfg` (`[skylanders]`) or **Settings → Controls**.

## Verified

On Linux (Wine 11.21, on an Xvfb display): attract mode and its videos play,
four coins start a game, element and Skylander selection respond to touches
at the cursor, and shooting enemies scores points. Escape exits cleanly.
Not yet verified: controller cursor movement on real hardware, a full game to
the results screen, and the operator menu.
