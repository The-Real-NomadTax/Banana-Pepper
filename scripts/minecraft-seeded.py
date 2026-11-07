#!/usr/bin/env python3
"""Launch Minecraft Dungeons Arcade 3.07 with captured dongle seed tables.

The seeds come from scripts/minecraft-capture-seeds.ps1 (runtime/minecraft/seeds.json).
A runtime-only copy of the import-repaired dump gets:
  * the plaintext 64/128-byte seed tables written at 0x16d3d40 / 0x16d3d80,
  * the five file-reader table pointers that 0x4a4e70 would normally store,
  * a jump at 0x430487 over the cabinet HASP login/decrypt block.
Originals are never modified. The untouched `game` and `Patch` are bound at their
cabinet paths so the game's file-integrity test sees the files it expects.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import threading
import time

SOURCE_SHA256 = "663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b"
BASE = 0x400000  # both LOAD segments map file offset = VA - 0x400000
SEEDS = (("seed64", 0x16d3d40, 64), ("seed128", 0x16d3d80, 128))
POINTERS = {0x1eb7d20: 0x16d4220, 0x1eb7d40: 0x16d41e0, 0x1eb7d38: 0x16d3e00,
            0x1eb7d28: 0x16d3d80, 0x1eb7d30: 0x16d3d40}
CALL_SITE, RESUME = 0x430487, 0x430542
REGION_BYTE = 0x17bad20  # normally loaded from dongle memory by the skipped init block
DEFAULT_COUNTRY = 48     # value in the supplied cabinet's own gamelog
FACTORY_SETUP_FLAG = 0x179d100  # 1 once factory setup has been completed
# Cached SysInfoGameCabinetType (0x5bba80). The skipped init block reads it from dongle
# memory offset 0x29; left at 0 no layout script matches, so player panels and the
# level-start flow never appear. 1 = four-player cabinet (TeknoParrot "4 PLAYERS"),
# 12 = two-player, 13 = one-player.
CABINET_TYPE = 0x182a4ac
DEFAULT_CABINET_TYPE = 1
# Card stock (0x441340): cards this cabinet may still dispense, normally read from dongle
# memory offset 0x32 (capped at 7000). Left at 0 the game locks every player slot with
# "GAME LOCKED / ALL DISPENSERS REPORT ERROR" and hides player panels and the HUD.
# Dongle reads/writes fail on the desktop, so the in-memory value is kept for the session.
CARD_STOCK = 0x179dee4
DEFAULT_CARD_STOCK = 7000
# (va, original bytes, replacement) applied to the runtime copy only.
OFFLINE_PATCHES = (
    # Dongle heartbeat (hasp_get_info): treat as success instead of counting to abort.
    (0x4a458d, bytes.fromhex("7529"), bytes.fromhex("9090")),
    # Dongle session check (hasp status): return 0.
    (0x4a44e0, bytes.fromhex("53488d"), bytes.fromhex("31c0c3")),
    # Region setter: keep the byte in memory instead of writing it to dongle memory
    # (jump to the plain setter at 0x4a5420).
    (0x4a5440, bytes.fromhex("8b15fa5831"), bytes.fromhex("e9dbffffff")),
    # Factory-setup flag loader (reads the flag from dongle memory): return success and
    # keep the preset in-image flag.
    (0x438bc0, bytes.fromhex("488d35"), bytes.fromhex("31c0c3")),
    # Four-player cabinet type 1 asks the I/O layer to wait for two physical RIO2
    # boards (IDs 0x1e/0x1f). The desktop has none, so skip that board-count request
    # and keep the cabinet's four-player layout.
    (0x42466c, bytes.fromhex("741a"), bytes.fromhex("9090")),
    # NetworkInit: continue when SysGetCryptoSeed() can't read dongle memory (online-only key).
    (0x490f79, bytes.fromhex("782d"), bytes.fromhex("9090")),
)
LOGO_TRIGGER_NAME = 0x430490  # inside the skipped HASP block (CALL_SITE..RESUME)


TRANSITION_OUT_DONE_NAME = 0x11a13cb


def transition_done(address):
    # Replace the blocking tail (mov rsi, cb; lea rdx, name; xor edi, edi; jmp 0x5b5020)
    # of a scene-load-with-swipe routine with SetTrigger(0, "TRANSITION_OUT_DONE").
    code = (b"\x48\x8d\x35" + struct.pack("<i", TRANSITION_OUT_DONE_NAME - (address + 7))
            + b"\x31\xff\xe9" + struct.pack("<i", 0x4ac960 - (address + 14)))
    return code + b"\x90" * (21 - len(code))


TRANSITION_PATCHES = (
    # The swipe ("transition" scene) scripts never run on this desktop runtime, so the
    # two load-with-swipe variants waited forever for TRANSITION_OUT_DONE (first seen at
    # the Mojang screen). Keep the native scene unload/load and notifications; only the
    # final blocking wait becomes SetTrigger(TRANSITION_OUT_DONE).
    (0x50e4f6, bytes.fromhex("48c7c600505b00488d15c72ec90031ffe9156b0a00"), transition_done(0x50e4f6)),
    (0x50e710, bytes.fromhex("48c7c600505b00488d15ad2cc90031ffe9fb680a00"), transition_done(0x50e710)),
    # Level select: the native game-select loop (0x433110) waits for GAMESEL_DONE, which
    # only the "transition" scene's logo swipe sets. That scene is not run here, so the
    # game froze at level select after Ready. Wait instead for LOGO_TRANSITION, which the
    # level-select script sets once a level is chosen and all joined players are ready.
    # The name lives in the unreachable HASP block skipped at CALL_SITE.
    (LOGO_TRIGGER_NAME, bytes.fromhex("1c03000031c0488d6c2420e8c0450700"), b"LOGO_TRANSITION\0"),
    (0x433110, bytes.fromhex("488d35f243d400"),
     b"\x48\x8d\x35" + struct.pack("<i", LOGO_TRIGGER_NAME - 0x433117)),
)


# The cabinet ran vsync'd at 60 Hz and the game's UI timing assumes it. On faster
# monitors (e.g. 165 Hz) panels, the HUD and title cards break, so by default the game
# runs inside a gamescope window that presents at exactly 60 Hz.
GAMESCOPE_HZ = 60
SET_DISPLAY = ("import os, sys; a = [os.environ['DISPLAY'] if x == '@DISPLAY@' else x "
               "for x in sys.argv[1:]]; os.execvp(a[0], a)")


def wrap_gamescope(command, resolution, disabled):
    gamescope = shutil.which("gamescope")
    if disabled or not gamescope:
        if not disabled:
            print("gamescope not found: running at the monitor's refresh rate; "
                  "UI may break above 60 Hz", flush=True)
        return command
    width, height = resolution.split("x")
    command = list(command)
    # gamescope starts its own Xwayland and picks the display number at launch.
    index = command.index("DISPLAY", command.index("--setenv"))
    command[index + 1] = "@DISPLAY@"
    print(f"Running in a {GAMESCOPE_HZ} Hz gamescope window ({resolution})", flush=True)
    # ARCADE_GAMESCOPE_BACKEND=headless runs unattended tests on a private display.
    backend = os.environ.get("ARCADE_GAMESCOPE_BACKEND")
    return ([gamescope, "-W", width, "-H", height, "-w", width, "-h", height,
             "-r", str(GAMESCOPE_HZ)] + (["--backend", backend] if backend else [])
            + ["--", sys.executable, "-c", SET_DISPLAY] + command)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def seeded_image(prepared, seeds, country=DEFAULT_COUNTRY, native_transitions=False,
                 cabinet_type=DEFAULT_CABINET_TYPE):
    image = bytearray(prepared)
    if not 0 < country < 0xff:
        raise RuntimeError("Country code must be 1-254")
    if not 1 <= cabinet_type <= 13:
        raise RuntimeError("Cabinet type must be 1-13")
    if image[CABINET_TYPE - BASE:CABINET_TYPE - BASE + 4] != bytes(4):
        raise RuntimeError("Unexpected bytes at the cabinet-type cache")
    image[CABINET_TYPE - BASE:CABINET_TYPE - BASE + 4] = struct.pack("<I", cabinet_type)
    if image[CARD_STOCK - BASE:CARD_STOCK - BASE + 4] != bytes(4):
        raise RuntimeError("Unexpected bytes at the card-stock counter")
    image[CARD_STOCK - BASE:CARD_STOCK - BASE + 4] = struct.pack("<I", DEFAULT_CARD_STOCK)
    image[REGION_BYTE - BASE] = country
    image[FACTORY_SETUP_FLAG - BASE] = 1
    if image[CALL_SITE - BASE] != 0xE8:
        raise RuntimeError("Unexpected bytes at the HASP init call site")
    for name, va, size in SEEDS:
        data = bytes.fromhex(seeds[name])
        if len(data) != size:
            raise RuntimeError(f"{name} must be {size} bytes")
        image[va - BASE:va - BASE + size] = data
    for va, value in POINTERS.items():
        image[va - BASE:va - BASE + 8] = struct.pack("<Q", value)
    patches = OFFLINE_PATCHES + (() if native_transitions else TRANSITION_PATCHES)
    for va, original, replacement in patches:
        if image[va - BASE:va - BASE + len(original)] != original:
            raise RuntimeError(f"Unexpected bytes at {va:#x}")
        image[va - BASE:va - BASE + len(replacement)] = replacement
    image[CALL_SITE - BASE:CALL_SITE - BASE + 5] = b"\xe9" + struct.pack("<i", RESUME - (CALL_SITE + 5))
    return bytes(image)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_directory", type=Path)
    parser.add_argument("--seeds", type=Path)
    parser.add_argument("--project-root", type=Path, help="project containing runtime data and the built card adapter")
    parser.add_argument("--timeout", type=int, help="stop after N seconds (testing)")
    parser.add_argument("--no-gamescope", action="store_true",
                        help="draw straight to the desktop instead of a 60 Hz gamescope window "
                             "(on monitors faster than 60 Hz the game's UI then breaks)")
    parser.add_argument("--resolution", default="1920x1080",
                        help="game render size (default: cabinet-native 1920x1080)")
    parser.add_argument("--country", type=int, default=DEFAULT_COUNTRY, help="cabinet country/region code")
    parser.add_argument("--cabinet-type", type=int, default=DEFAULT_CABINET_TYPE,
                        help="cabinet type: 1 = four players (default), 12 = two, 13 = one")
    parser.add_argument("--native-transitions", action="store_true",
                        help="restore original swipe transitions (currently stall on desktop)")
    parser.add_argument("--no-joysticks", action="store_true", help="don't create virtual cabinet joysticks")
    parser.add_argument("--no-keyboard", action="store_true", help="create joysticks but don't map the keyboard")
    parser.add_argument("--no-host-keyboard", action="store_true",
                        help="ignore the host keyboard; only runtime/minecraft-seeded/key-events drives input "
                             "(for unattended runs on a private display)")
    parser.add_argument("--no-cards", action="store_true", help="use original physical QR scanners")
    parser.add_argument("--card", default="a0000000", help="eight-character QR code scanned with F1-F4 (default: Sun's Grace)")
    parser.add_argument("--gdb", type=Path, help="run under gdb with this batch script (diagnostics)")
    args = parser.parse_args()
    scripts = Path(__file__).resolve().parent
    project = (args.project_root or scripts.parent).resolve()
    game = args.game_directory.resolve()
    runner = module("minecraft_runner", scripts / "minecraft-run.py")
    prepare = module("minecraft_prepare", scripts / "minecraft-prepare.py")
    source = (game / "gamedump.exec").read_bytes()
    if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
        raise RuntimeError("Unsupported gamedump.exec build")
    seeds_path = args.seeds or project / "runtime/minecraft/seeds.json"
    if not seeds_path.is_file():
        raise RuntimeError(f"Captured Minecraft seeds are missing: {seeds_path}. See docs/minecraft-seed-capture.md.")
    seeds = json.loads(seeds_path.read_text())
    if seeds.get("game_sha256") != SOURCE_SHA256:
        raise RuntimeError("Seed file was captured from a different build")
    libraries = project / "runtime/minecraft/lib"
    if not libraries.is_dir():
        raise RuntimeError("Run --setup-minecraft-libs first")
    output = project / "runtime/minecraft-seeded"
    output.mkdir(parents=True, exist_ok=True)
    with (output / "session.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        cards = module("minecraft_cards", scripts / "minecraft-cards.py")
        cards.validate(1, args.card)
        if not args.no_cards:
            library = project / "target/debug/libarcade_minecraft_io.so"
            if not library.is_file():
                raise RuntimeError("Build the card adapter first: cargo build -p arcade-minecraft-io --offline")
            shutil.copyfile(library, output / "cards.so")
            (output / "card-events").write_text("")
            (output / "key-events").write_text("")
        image = output / "game.seeded"
        image.write_bytes(seeded_image(prepare.prepare(source), seeds, args.country,
                                      args.native_transitions, args.cabinet_type))
        image.chmod(0o755)
        placeholder = output / "game"  # sandbox() binds this path; overridden below
        if not placeholder.exists():
            placeholder.symlink_to("game.seeded")
        (output / "home").mkdir(exist_ok=True)
        if not (output / "pmuser").exists():
            shutil.copytree(game / "pmuser", output / "pmuser")
        command = runner.sandbox(game, output)
        # sandbox() binds the repaired image at /pm/g7/minecraft/game; later binds
        # win, so the original protected files take those paths instead.
        command += ["--ro-bind", str(game / "game"), "/pm/g7/minecraft/game",
                    "--ro-bind", str(game / "Patch"), "/pm/g7/minecraft/Patch",
                    "--ro-bind", str(libraries), "/work/lib"]
        runner.display_options(command)
        for device in ("/dev/nvidia0", "/dev/nvidiactl", "/dev/nvidia-modeset", "/dev/nvidia-uvm"):
            if Path(device).exists():
                command += ["--dev-bind", device, device]
        for path in ("/sys", "/proc/driver/nvidia", "/run/udev"):
            if Path(path).exists():
                command += ["--ro-bind", path, path]
        pads, stop = [], threading.Event()
        if not args.no_joysticks:
            joysticks = module("minecraft_joysticks", scripts / "minecraft-joysticks.py")
            pads = joysticks.make_pads(4)
            time.sleep(0.5)  # let udev create the nodes and tag them
            command += ["--dir", "/dev/input"]
            for pad in pads:
                command += ["--dev-bind", pad.device.path, pad.device.path]
            # Feed host keys directly to the adapter as well as SDL: XWayland
            # keyboard delivery is unreliable for this cabinet's old SDL build.
            # The adapter merges matching events; do not also forward via uinput.
            def native_key(source, value):
                with (output / "key-events").open("a") as events:
                    events.write(f"{source} {value}\n")
            threading.Thread(target=joysticks.run, args=(pads, not (args.no_keyboard or args.no_host_keyboard)),
                             kwargs={"stop": stop, "native_key": None if args.no_cards else native_key},
                             daemon=True).start()
            print("Virtual joysticks: " + ", ".join(p.device.path for p in pads), flush=True)
        private_bin = output / "bin"
        private_bin.mkdir(exist_ok=True)
        for name in ("nvidia-settings", "xrandr", "reboot", "shutdown"):
            inert = private_bin / name
            inert.write_text("#!/bin/sh\nexit 0\n")
            inert.chmod(0o755)
            executable = shutil.which(name)
            if executable and name in ("nvidia-settings", "xrandr"):
                command += ["--ro-bind", str(inert), executable]
        command += ["--setenv", "PATH", "/work/bin:/usr/bin:/bin"]
        if not args.no_cards:
            command += ["--setenv", "LD_PRELOAD", "/work/cards.so",
                        "--setenv", "ARCADE_MINECRAFT_CARDS", "1",
                        "--setenv", "ARCADE_MINECRAFT_KEYBOARD", "0" if args.no_keyboard else "1",
                        "--setenv", "ARCADE_MINECRAFT_CARD", args.card]
            print(f"Virtual card scanners enabled. F1-F4 scan {args.card} for P1-P4; scripts/minecraft-cards.py sends other cards.", flush=True)
            if not args.no_keyboard:
                print("Keyboard: 5-8 coins, 1-4 player action/Start; P1 arrows + Z/X/C, P2 IJKL + U/O/P. Click Minecraft to focus.", flush=True)
        game_args = ["/work/game.seeded", f"-f{args.resolution}", "-n"]
        if args.gdb:
            shutil.copyfile(args.gdb, output / "diag.gdb")
            game_args = ["gdb", "-batch", "-nx", "-nh", "-x", "/work/diag.gdb", "--args"] + game_args
        command += game_args
        command = wrap_gamescope(command, args.resolution, args.no_gamescope)
        print(f"Launching seeded Minecraft. Logs: {output}/runner.log and {output}/pmuser/gamelog.txt", flush=True)
        with (output / "runner.log").open("w") as log:
            process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
            try:
                status = process.wait(timeout=args.timeout)
            except subprocess.TimeoutExpired:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                status = "timeout"
        stop.set()
        for pad in pads:
            pad.close()
        print(f"Exit: {status}")
        return 0 if status in (0, "timeout") else 1


if __name__ == "__main__":
    raise SystemExit(main())
