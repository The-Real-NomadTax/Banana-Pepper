#!/usr/bin/env python3
"""Aliens desktop sandbox. Original assets are read-only; saves are private copies."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

from arcade_audio import sdl_audio_args
from arcade_settings import guest_env_args, resolution


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--games-root', type=Path, required=True)
    parser.add_argument('--launch', action='store_true')
    parser.add_argument('--debug', action='store_true')
    parser.add_argument('--plaintext-assets', action='store_true', help='diagnostic substitution of supplied readable definition copies')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    game = args.games_root.resolve() / 'linux based/Aliens Armageddon'
    output = root / 'runtime/aliens'
    output.mkdir(parents=True, exist_ok=True)
    if hashlib.sha256((game / 'game').read_bytes()).hexdigest() != '297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db':
        raise SystemExit('Unsupported Aliens build')
    # The prepared image varies with controls.cfg; the Rust preparer records its hash.
    recorded = output / 'game-native.sha256'
    expected = recorded.read_text().strip() if recorded.is_file() else ''
    if not expected or hashlib.sha256((output / 'game-native').read_bytes()).hexdigest() != expected:
        raise SystemExit('Prepare the hash-verified Aliens runtime using the Rust CLI first')
    with (output / 'session.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit('An Aliens session is already running')
        if not (output / 'pmuser').exists():
            shutil.copytree(game / 'g6/aa/pmuser', output / 'pmuser', symlinks=True)
        game_log = output / 'pmuser/gamelog.txt'
        log_start = game_log.stat().st_size if game_log.exists() else 0
        shutil.copyfile(root / 'target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so', output / 'shim.so.preparing')
        (output / 'shim.so.preparing').replace(output / 'shim.so')
        display = os.environ.get('DISPLAY', '')
        number = display.removeprefix(':').split('.')[0]
        if not display.startswith(':') or not number.isdigit():
            raise SystemExit('A local desktop X11 display is required')
        command = ['bwrap', '--unshare-all', '--die-with-parent', '--clearenv', '--ro-bind', '/usr', '/usr']
        for name in ['lib', 'lib32', 'lib64', 'bin']:
            path = Path('/') / name
            if path.is_symlink():
                command += ['--symlink', str(path.readlink()), str(path)]
            elif path.exists():
                command += ['--ro-bind', str(path), str(path)]
        command += ['--dev', '/dev', '--proc', '/proc', '--tmpfs', '/tmp',
                    '--ro-bind', str(game), '/pm', '--ro-bind', str(output / 'game-native'), '/pm/game',
                    '--bind', str(output), '/work', '--bind', str(output / 'pmuser'), '/pm/g6/aa/pmuser',
                    '--ro-bind', str(output / 'shim.so'), '/shim.so',
                    '--ro-bind', str(root / 'runtime/jurassic/libs'), '/runtime-libs',
                    '--ro-bind-try', '/etc/ld.so.cache', '/etc/ld.so.cache',
                    '--ro-bind-try', '/etc/fonts', '/etc/fonts',
                    '--setenv', 'DISPLAY', display, '--setenv', 'PATH', '/work/bin:/usr/bin:/bin',
                    '--setenv', 'HOME', '/tmp', '--setenv', 'LD_LIBRARY_PATH', '/runtime-libs:/usr/lib32',
                    '--setenv', 'ARCADE_ALIENS', '1',
                    # Unique class lets the shim find this window for Esc / close-button exit.
                    '--setenv', 'SDL_VIDEO_X11_WMCLASS', 'ArcadeFoundryAliens',
                    '--setenv', 'SDL_VIDEO_X11_XRANDR', '0', '--setenv', 'SDL_VIDEO_X11_VIDMODE', '0',
                    '--setenv', 'SDL_VIDEO_X11_DGAMOUSE', '0', '--chdir', '/pm']
        # The game's statically linked SDL 1.2 includes a PulseAudio driver ("pulse").
        if args.launch:
            audio_args, audio_driver = sdl_audio_args('pulse')
        else:
            audio_args, audio_driver = ['--setenv', 'SDL_AUDIODRIVER', 'dummy'], 'dummy'
        command += audio_args
        # Fullscreen, vsync and renderer choices from settings.cfg (launch only).
        if args.launch:
            command += guest_env_args()
        # Use supplied development definitions instead of encrypted prod copies.
        for name in ['aud', 'adj', 'conf', 'attract', 'cpmgui', 'online', 'locales', 'translation', 'ttf']:
            source = game / 'g6/aa/data' / name
            if source.is_dir() and args.plaintext_assets:
                command += ['--ro-bind', str(source), f'/pm/g6/aa/data/prod/{name}']
        socket = f'/tmp/.X11-unix/X{number}'
        command += ['--ro-bind', socket, socket]
        cookie = Path(os.environ.get('XAUTHORITY', str(Path.home() / '.Xauthority')))
        if cookie.is_file():
            command += ['--ro-bind', str(cookie), '/tmp/game.Xauthority', '--setenv', 'XAUTHORITY', '/tmp/game.Xauthority']
        for device in ['/dev/dri', '/dev/nvidia0', '/dev/nvidiactl', '/dev/nvidia-modeset', '/dev/nvidia-uvm']:
            if Path(device).exists():
                command += ['--dev-bind', device, device]
        for path in ['/sys', '/proc/driver/nvidia']:
            if Path(path).exists():
                command += ['--ro-bind', path, path]
        tools = output / 'bin'
        tools.mkdir(exist_ok=True)
        for tool in ['nvidia-settings', 'xrandr', 'reboot', 'shutdown']:
            target = tools / tool
            target.write_text('#!/bin/sh\nexit 0\n')
            target.chmod(0o755)
        for stale in output.glob('window-*.png'):
            stale.unlink()
        (output / 'window-geometry.txt').unlink(missing_ok=True)
        size = resolution() if args.launch else '1360x768'
        run = f'LD_PRELOAD=/shim.so /pm/game -f{size}'
        if not args.launch:
            run = 'timeout -k 2s 45s strace -f -s 180 -o /work/startup.strace -E LD_PRELOAD=/shim.so /pm/game -f1360x768'
        if args.debug:
            shutil.copyfile(root / 'scripts/aliens-probe.gdb', output / 'probe.gdb')
            run = 'timeout -k 2s 45s gdb -batch -x /work/probe.gdb --args /pm/game -f1360x768'
        guest = output / 'guest.sh'
        guest.write_text('#!/bin/bash\nset -u\n' + run + ' > /work/game.log 2>&1 &\npid=$!\n'
                         'for frame in 10 20 30 40; do\n sleep 10\n'
                         ' window=$(xdotool search --onlyvisible --name "[Aa][Ll][Ii][Ee][Nn][Ss]" | head -1)\n'
                         ' if [[ -n "$window" ]]; then\n'
                         '  xdotool getwindowname "$window" > /work/window-title.txt\n'
                         '  xdotool getwindowgeometry --shell "$window" > /work/window-geometry.txt\n'
                         '  import -window "$window" /work/window-$frame.png 2>/work/capture.log || true\n fi\n'
                         ' kill -0 "$pid" 2>/dev/null || break\ndone\nwait "$pid"\n')
        command += ['/bin/bash', '/work/guest.sh']
        print(f'Aliens {"desktop launch" if args.launch else "45-second startup probe"}; audio: {audio_driver}; logs: {output}', flush=True)
        status = subprocess.run(command, check=False).returncode
        (output / 'exit-code.txt').write_text(str(status) + '\n')
        initialization = game_log.read_bytes()[log_start:].decode(errors='replace') if game_log.exists() else ''
        runner_log = (output / 'game.log').read_text(errors='replace') if (output / 'game.log').exists() else ''
        # The window manager closes the client after the close button is used;
        # Xlib then reports the lost connection and exits non-zero.
        closed_by = ('escape' if '[aliens] Esc pressed; exiting' in runner_log else
                     'window' if 'X connection to' in runner_log or 'XIO:' in runner_log else None)
        (output / 'initialization.log').write_text(initialization)
        (output / 'result.json').write_text(json.dumps({
            'exit_code': status, 'debugger': args.debug, 'plaintext_substitution': args.plaintext_assets,
            'window_captured': any(output.glob('window-*.png')), 'gameplay_verified': False,
            'audio_driver': audio_driver, 'closed_by': closed_by, 'dongle_initialization_failed': 'DongleInitialize() FAILED!' in initialization,
            'rtdt_definitions_failed': 'Error Loading RTDT Defs File' in initialization,
            'segmentation_fault': 'FATAL signal SIGSEGV' in initialization, 'source_sha256': hashlib.sha256((game / 'game').read_bytes()).hexdigest(),
            'log': str(output / 'game.log'),
        }, indent=2) + '\n')
        if args.launch and closed_by:
            print(f'Aliens closed ({"Esc" if closed_by == "escape" else "window close button"}).', flush=True)
            return 0
        return 0 if status == 124 and not args.launch else status


if __name__ == '__main__':
    raise SystemExit(main())
