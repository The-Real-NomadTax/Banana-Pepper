#!/usr/bin/env python3
"""Explicit, bounded decoder experiment for the hash-verified Minecraft dump.

The cabinet login/ID block is skipped only in the debugger's loaded image.
This does not provide decoded seed tables or certify working gameplay.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


GDB = r'''set pagination off
set confirm off
set startup-with-shell off
set print thread-events off
set disable-randomization off
break *0x430487
commands
silent
set {unsigned long}0x1eb7d20=$rdi
set {unsigned long}0x1eb7d40=$rsi
set {unsigned long}0x1eb7d38=$rdx
set {unsigned long}0x1eb7d28=$rcx
set {unsigned long}0x1eb7d30=$r8
set $pc=0x430542
@SEEDS@printf "EXPERIMENT: cabinet login/ID skipped; file-reader pointers retained\n"
continue
end
break *0x4e534f
commands
silent
printf "MAP_KEY_NAME=%s\n", $r15
dump binary memory /work/map-key.bin $rsp+0x60 $rsp+0x80
continue
end
break *0x4e53b4
commands
silent
printf "MAP_IV_NAME=%s\n", $r15
dump binary memory /work/map-iv.bin $rsp+0x50 $rsp+0x60
continue
end
break *0x4e53fc
commands
silent
printf "MAP_AES_RESULT=%d LENGTH=%u\n", $eax, *(unsigned int*)($rsp+0x50)
set $mapout=*(unsigned long*)($rsp+0x58)
if $eax == 0 && *(unsigned int*)($rsp+0x50) >= 584
  dump binary memory /work/map-decoded.bin $mapout $mapout+584
end
continue
end
run
bt 10
quit
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('game_directory', type=Path)
    parser.add_argument('--seeds', type=Path, help='captured seeds JSON (scripts/minecraft-capture-seeds.ps1)')
    parser.add_argument('--timeout', type=int, default=35)
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[1]
    game = args.game_directory.resolve()
    runner = module('minecraft_runner', project / 'scripts/minecraft-run.py')
    prepare = module('minecraft_prepare', project / 'scripts/minecraft-prepare.py')
    source = (game / 'gamedump.exec').read_bytes()
    # prepare validates the complete source hash before any debugger addresses
    # are used. All instruction bytes in the prepared file are preserved.
    prepared = prepare.prepare(source)
    output = project / 'runtime/minecraft-asset-probe'
    libraries = project / 'runtime/minecraft/lib'
    normal_runtime = project / 'runtime/minecraft/game'
    normal_hash = hashlib.sha256(normal_runtime.read_bytes()).hexdigest() if normal_runtime.exists() else None
    if not libraries.is_dir():
        raise RuntimeError('Prepare Minecraft libraries with the normal launcher first')
    output.mkdir(parents=True, exist_ok=True)
    with (output / 'session.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        (output / 'game').write_bytes(prepared)
        (output / 'game').chmod(0o755)
        (output / 'home').mkdir(exist_ok=True)
        if not (output / 'pmuser').exists():
            shutil.copytree(game / 'pmuser', output / 'pmuser')
        for name in ('map-key.bin', 'map-iv.bin', 'map-decoded.bin'):
            (output / name).unlink(missing_ok=True)
        seed_lines = ''
        if args.seeds:
            seeds = json.loads(args.seeds.read_text())
            if seeds.get('game_sha256') != hashlib.sha256(source).hexdigest():
                raise RuntimeError('Seed file was captured from a different build')
            for name, va, size in (('seed64', 0x16d3d40, 64), ('seed128', 0x16d3d80, 128)):
                data = bytes.fromhex(seeds[name])
                if len(data) != size:
                    raise RuntimeError(f'{name} must be {size} bytes')
                (output / f'{name}.bin').write_bytes(data)
                seed_lines += f'restore /work/{name}.bin binary {va:#x}\n'
            seed_lines += 'printf "SEEDS: captured plaintext tables restored\\n"\n'
        (output / 'probe.gdb').write_text(GDB.replace('@SEEDS@', seed_lines))
        command = runner.sandbox(game, output)
        command += ['--ro-bind', str(libraries), '/work/lib']
        runner.display_options(command)
        for device in ('/dev/nvidia0', '/dev/nvidiactl', '/dev/nvidia-modeset', '/dev/nvidia-uvm'):
            if Path(device).exists():
                command += ['--dev-bind', device, device]
        for path in ('/sys', '/proc/driver/nvidia'):
            if Path(path).exists():
                command += ['--ro-bind', path, path]
        # Keep cabinet shell helpers inert, including absolute NVIDIA/XRandR
        # paths, while retaining the user's desktop display for any game window.
        private_bin = output / 'bin'
        private_bin.mkdir(exist_ok=True)
        for name in ('nvidia-settings', 'xrandr', 'reboot', 'shutdown'):
            inert = private_bin / name
            inert.write_text('#!/bin/sh\nexit 0\n')
            inert.chmod(0o755)
            executable = shutil.which(name)
            if executable and name in ('nvidia-settings', 'xrandr'):
                command += ['--ro-bind', str(inert), executable]
        command += ['--setenv', 'PATH', '/work/bin:/usr/bin:/bin']
        print(f'Explicit decoder experiment; no working-game claim. Logs: {output}', flush=True)
        with (output / 'probe.log').open('w') as log:
            try:
                process = subprocess.run(command + ['gdb', '-batch', '-nx', '-nh', '-x', '/work/probe.gdb',
                    '--args', '/pm/g7/minecraft/game', '-f1280x720', '-n'],
                    stdout=log, stderr=subprocess.STDOUT, timeout=args.timeout)
                status = process.returncode
            except subprocess.TimeoutExpired:
                status = 'timeout'
        trace = (output / 'probe.log').read_text(errors='replace')
        decoded = output / 'map-decoded.bin'
        report = {
            'source_sha256': hashlib.sha256(source).hexdigest(),
            'prepared_sha256': hashlib.sha256(prepared).hexdigest(),
            'cabinet_login_skipped_in_debugger': True,
            'original_unchanged': hashlib.sha256((game / 'gamedump.exec').read_bytes()).digest() == hashlib.sha256(source).digest(),
            'normal_runtime_unchanged': normal_hash == (hashlib.sha256(normal_runtime.read_bytes()).hexdigest() if normal_runtime.exists() else None),
            'debugger_exit_code': status,
            'map_captured': decoded.exists(),
            'segmentation_fault': 'SIGSEGV' in trace,
            'gameplay_verified': False,
        }
        # Independent AES check is optional; the game remains the primary probe.
        if decoded.exists():
            try:
                from cryptography.hazmat.primitives.ciphers import Cipher, algorithms
                from cryptography.hazmat.decrepit.ciphers.modes import OFB
                actual = decoded.read_bytes()
                key = (output / 'map-key.bin').read_bytes()
                iv = (output / 'map-iv.bin').read_bytes()
                independent = Cipher(algorithms.AES(key), OFB(iv)).decryptor().update(
                    (game / 'eFileMaps/fileMap000').read_bytes())
                report['independent_aes_matches'] = independent == actual
            except ImportError:
                report['independent_aes_matches'] = None
        (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 0 if report['map_captured'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
