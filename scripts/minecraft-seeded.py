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
from pathlib import Path
import shutil
import struct
import subprocess
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
    # NetworkInit: continue when SysGetCryptoSeed() can't read dongle memory (online-only key).
    (0x490f79, bytes.fromhex("782d"), bytes.fromhex("9090")),
)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def seeded_image(prepared, seeds, country=DEFAULT_COUNTRY):
    image = bytearray(prepared)
    if not 0 < country < 0xff:
        raise RuntimeError("Country code must be 1-254")
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
    for va, original, replacement in OFFLINE_PATCHES:
        if image[va - BASE:va - BASE + len(original)] != original:
            raise RuntimeError(f"Unexpected bytes at {va:#x}")
        image[va - BASE:va - BASE + len(replacement)] = replacement
    image[CALL_SITE - BASE:CALL_SITE - BASE + 5] = b"\xe9" + struct.pack("<i", RESUME - (CALL_SITE + 5))
    return bytes(image)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_directory", type=Path)
    parser.add_argument("--seeds", type=Path)
    parser.add_argument("--timeout", type=int, help="stop after N seconds (testing)")
    parser.add_argument("--resolution", default="1280x720")
    parser.add_argument("--country", type=int, default=DEFAULT_COUNTRY, help="cabinet country/region code")
    parser.add_argument("--no-joysticks", action="store_true", help="don't create virtual cabinet joysticks")
    parser.add_argument("--no-keyboard", action="store_true", help="create joysticks but don't map the keyboard")
    parser.add_argument("--gdb", type=Path, help="run under gdb with this batch script (diagnostics)")
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[1]
    game = args.game_directory.resolve()
    runner = module("minecraft_runner", project / "scripts/minecraft-run.py")
    prepare = module("minecraft_prepare", project / "scripts/minecraft-prepare.py")
    source = (game / "gamedump.exec").read_bytes()
    if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
        raise RuntimeError("Unsupported gamedump.exec build")
    seeds_path = args.seeds or project / "runtime/minecraft/seeds.json"
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
        image = output / "game.seeded"
        image.write_bytes(seeded_image(prepare.prepare(source), seeds, args.country))
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
            joysticks = module("minecraft_joysticks", project / "scripts/minecraft-joysticks.py")
            pads = joysticks.make_pads(4)
            time.sleep(0.5)  # let udev create the nodes and tag them
            command += ["--dir", "/dev/input"]
            for pad in pads:
                command += ["--dev-bind", pad.device.path, pad.device.path]
            threading.Thread(target=joysticks.run, args=(pads, not args.no_keyboard),
                             kwargs={"stop": stop}, daemon=True).start()
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
        game_args = ["/work/game.seeded", f"-f{args.resolution}", "-n"]
        if args.gdb:
            shutil.copyfile(args.gdb, output / "diag.gdb")
            game_args = ["gdb", "-batch", "-nx", "-nh", "-x", "/work/diag.gdb", "--args"] + game_args
        command += game_args
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
