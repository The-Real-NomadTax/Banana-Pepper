#!/usr/bin/env python3
"""Jurassic sandbox harness for the Rust-prepared desktop and diagnostic images.

--launch runs the desktop profile without a debugger or timeout. Diagnostic mode
uses explicit GDB skips and bounded captures. Both require supplied plaintext
script backups. Game source files and host display settings remain unchanged.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

from arcade_audio import sdl_audio_args
from arcade_settings import guest_env_args, pad_args, resolution


def prepare_save_mounts(game, output):
    """Use one persistent save tree for original and materialized ZAR layouts."""
    saves = output / "pmuser"
    if not saves.exists():
        shutil.copytree(game / "pmuser", saves, symlinks=True)
    args = ["--bind", str(saves), "/pm/pmuser",
            "--bind", str(saves), "/pmuser"]
    # Folder sources use a link to /pmuser. Archives may materialize it as a
    # directory or omit it entirely (older converters skipped broken links).
    nested = game / "g5/dino/pmuser"
    if not nested.is_symlink():
        if not nested.exists():
            # A missing mountpoint cannot be created inside read-only /pm.
            # Rebuild just its parent in a private tmpfs, keeping assets read-only.
            args += ["--tmpfs", "/pm/g5/dino"]
            for child in sorted(nested.parent.iterdir()):
                args += ["--ro-bind", str(child), f"/pm/g5/dino/{child.name}"]
        args += ["--bind", str(saves), "/pm/g5/dino/pmuser"]
    return args


def stage_shader_overrides(game, output):
    """Keep color/bloom output conventions consistent in runtime-only copies."""
    overrides = []
    prefix = (b"out vec4 out_Color;\r\n#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n"
              b"#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif\r\n")
    for name in ["shader108", "shader108b", "shader108c_sk", "shader108d", "shader108d_sk"]:
        relative = Path(name) / "frag.gls"
        source = (game / "g5/shaders" / relative).read_bytes()
        if b"gl_FragData[0]" not in source or b"gl_FragData[1]" not in source:
            raise SystemExit(f"Unsupported shader variant: {relative}")
        patched = prefix + source.replace(b"gl_FragData[0]", b"out_Color").replace(b"gl_FragData[1]", b"out_Bloom")
        overrides.append((relative, patched))
    relative = Path("include/frag_bloom.gls")
    source = (game / "g5/shaders" / relative).read_bytes()
    if source.count(b"out vec4 out_Bloom;") != 1:
        raise SystemExit("Unsupported shared bloom shader")
    patched = source.replace(b"out vec4 out_Bloom;", b"#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif")
    overrides.append((relative, patched))
    mounted = []
    for relative, patched in overrides:
        destination = output / "shader-overrides" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(patched)
        mounted.append((destination, Path("/pm/g5/shaders") / relative))
    return mounted


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=60, help="capture duration: 10–600 seconds, in steps of 10")
    parser.add_argument("--desktop", action="store_true", help="use the desktop X11 display and available GPU")
    parser.add_argument("--launch", action="store_true", help="launch the prepared desktop profile without a debugger or timeout")
    parser.add_argument("--games-root", type=Path)
    options = parser.parse_args()
    if not 10 <= options.seconds <= 600 or options.seconds % 10:
        parser.error("--seconds must be 10–600 in steps of 10")
    root = Path(__file__).resolve().parents[1]
    games = options.games_root or root / "games"
    game = next((games / d for d in ["Jurassic Park 2015", "linux based/Jurassic Park 2015", "."]
                 if (games / d / "game_original").is_file() or (games / d / "game_legacy_patched").is_file()),
                games / "Jurassic Park 2015")
    runtime = root / "runtime/jurassic"
    image = runtime / ("game-desktop" if options.launch else "game-native")
    expected = "3cdb20b56f3a881232e81aa7dc16c0476060489acd7d3b838d5cc6c1ba6aa3ef"
    if options.launch:
        expected = "9fd96c7e1d14c8863c1e790545c21e9d81f6f72a6b8cb16686f25da0fac313bc"
    if not image.is_file() or hashlib.sha256(image.read_bytes()).hexdigest() != expected:
        raise SystemExit("Prepare the hash-verified --probe-jurassic-native image first.")
    shim = root / "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so"
    xvfb = root / "runtime/tools/xvfb/usr/bin/Xvfb"
    for path in [shim, xvfb, runtime / "libs"]:
        if not path.exists():
            raise SystemExit(f"Required diagnostic dependency missing: {path}")
    backups = game / "programs_dec"
    programs = list((game / "g5/dino/data/programs_enc").rglob("*.txt"))
    if not programs:
        raise SystemExit("No game script files found.")
    for program in programs:
        relative = program.relative_to(game / "g5/dino/data/programs_enc")
        backup = backups / relative
        if not backup.is_file() or not backup.read_bytes().startswith(b"//\n"):
            raise SystemExit(f"Missing/unsupported plaintext backup: {relative}")

    with (runtime / "session.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        output = runtime / ("desktop" if options.launch else "exploration")
        output.mkdir(exist_ok=True)
        save_mounts = prepare_save_mounts(game, output)
        for name in ["jurassic-explore.gdb", "jurassic-explore-guest.sh"]:
            shutil.copyfile(root / "scripts" / name, output / name)
        rules = output / "usb.rules"
        rules.write_text("# Private test filesystem; cabinet USB automount disabled.\n")
        tools = output / "bin"
        tools.mkdir(exist_ok=True)
        settings = tools / "nvidia-settings"
        settings.write_text("#!/bin/sh\n# Cabinet display settings are intentionally ignored.\nexit 0\n")
        settings.chmod(0o755)
        shader_overrides = stage_shader_overrides(game, output)
        staged_shim = output / "shim.so.preparing"
        shutil.copyfile(shim, staged_shim)
        staged_shim.replace(output / "shim.so")
        shim = output / "shim.so"
        args = [
            "bwrap", "--unshare-all",
            "--die-with-parent", "--clearenv", "--ro-bind", "/usr", "/usr",
        ]
        for name in ["lib", "lib32", "lib64", "bin"]:
            path = Path("/") / name
            if path.is_symlink():
                args += ["--symlink", str(path.readlink()), str(path)]
            elif path.exists():
                args += ["--ro-bind", str(path), str(path)]
        args += [
            "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--ro-bind", str(game), "/pm",
            "--ro-bind", str(image), "/pm/game",
            "--ro-bind", str(runtime / "libs"), "/runtime-libs",
            "--ro-bind", str(shim), "/shim.so",
            "--setenv", "LD_LIBRARY_PATH", "/runtime-libs",
            "--setenv", "LD_BIND_NOW", "1",
            "--setenv", "ARCADE_JURASSIC", "1",
            "--setenv", "PATH", "/work/bin:/usr/bin:/bin",
            "--bind", str(output), "/work",
            *save_mounts,
            "--dir", "/etc/udev/rules.d",
            "--ro-bind", str(rules), "/etc/udev/rules.d/10-usb-automount.rules",
            "--ro-bind", str(xvfb), "/Xvfb",
            "--ro-bind", str(backups), "/pm/g5/dino/data/programs_enc",
        ]
        for source, destination in shader_overrides:
            args += ["--ro-bind", str(source), str(destination)]
        if options.launch:
            args += ["--setenv", "ARCADE_JURASSIC_LAUNCH", "1"]
        else:
            args = ["timeout", "-k", "1s", f"{options.seconds + 15}s"] + args
        # Host SDL 1.2 (sdl12-compat) forwards to SDL2/SDL3, whose Pulse driver is "pulseaudio".
        if options.launch and options.desktop:
            audio_args, audio_driver = sdl_audio_args("pulseaudio")
        else:
            audio_args, audio_driver = ["--setenv", "SDL_AUDIODRIVER", "dummy"], "dummy"
        args += audio_args
        if options.desktop:
            display = os.environ.get("DISPLAY", "")
            number = display.removeprefix(":").split(".")[0]
            if not display.startswith(":") or not number.isdigit():
                raise SystemExit("Desktop test requires a local X11 DISPLAY.")
            socket = Path(f"/tmp/.X11-unix/X{number}")
            if not socket.exists():
                raise SystemExit(f"Desktop X11 socket missing: {socket}")
            args += ["--ro-bind", str(socket), str(socket), "--setenv", "DISPLAY", display,
                     "--setenv", "ARCADE_JURASSIC_DESKTOP", "1"]
            # Key bindings from controls.cfg, prepared by the Rust launcher.
            keys = os.environ.get("ARCADE_JURASSIC_KEYS")
            if keys:
                args += ["--setenv", "ARCADE_JURASSIC_KEYS", keys]
            # Resolution, fullscreen, vsync and renderer from settings.cfg.
            if options.launch:
                args += ["--setenv", "ARCADE_RESOLUTION", resolution()] + guest_env_args() + pad_args()
            authority = Path(os.environ.get("XAUTHORITY", str(Path.home() / ".Xauthority")))
            if authority.is_file():
                args += ["--ro-bind", str(authority), "/tmp/game.Xauthority", "--setenv", "XAUTHORITY", "/tmp/game.Xauthority"]
            for device in ["/dev/dri", "/dev/nvidia0", "/dev/nvidiactl", "/dev/nvidia-modeset", "/dev/nvidia-uvm"]:
                if Path(device).exists():
                    args += ["--dev-bind", device, device]
            for directory in ["/sys", "/proc/driver/nvidia"]:
                if Path(directory).exists():
                    args += ["--ro-bind", directory, directory]
        args += ["--chdir", "/pm", "/bin/bash", "/work/jurassic-explore-guest.sh", str(options.seconds)]
        print(f"{'DESKTOP STARTUP' if options.launch else 'DEBUG EXPERIMENT'}: {len(programs)} plaintext backups; cabinet security initialization omitted; audio: {audio_driver}.", flush=True)
        print(f"{'Desktop' if options.desktop else 'Private-display'} captures and runtime log: {output}", flush=True)
        status = subprocess.run(args, check=False).returncode
        log = (output / ("game.log" if options.launch else "gdb.log")).read_text(errors="replace")
        windows = output / "windows.txt"
        result = {
            "mode": "desktop_startup" if options.launch else "debugger_exploration",
            "duration_requested_seconds": options.seconds,
            "exit_code": status,
            "gameplay_verified": False,
            "dongle_supported": False,
            "diagnostic_skips": [] if options.launch else ["GameInitDongle", "decryption of marked plaintext scripts"],
            "cabinet_security_initialization": "omitted; supplied plaintext assets used; no HASP decryption",
            "plaintext_program_backups": len(programs),
            "shader_overrides": [str(destination) for _, destination in shader_overrides],
            "audio_driver": audio_driver,
            "display": "desktop X11" if options.desktop else "private Xvfb",
            "window_titles": windows.read_text(errors="replace") if windows.exists() else "",
            "swap_buffers_observed": "SDL_GL_SwapBuffers" in log,
            "segmentation_fault": "SIGSEGV" in log,
            "captures": [str(output / f"screen-{second}.png") for second in range(10, options.seconds + 1, 10)
                         if (output / f"screen-{second}.png").exists()],
        }
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        return status


if __name__ == "__main__":
    raise SystemExit(main())
