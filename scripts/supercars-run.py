#!/usr/bin/env python3
"""Fast and Furious Super Cars desktop session under Wine + DXVK.

The game is a Windows build. This harness keeps a private Wine prefix in
runtime/supercars/prefix, maps the supplied game directory into it as
C:\\rawart (large assets are symlinks, small writable files such as prefs and
audits are private copies), installs the Banana pepper input adapter
(dinput8.dll, built from game-compat/super-cars/hook), and starts sdaemon.exe.
Original files are never written.

Rendering uses Wine's built-in D3D8 (WineD3D over OpenGL on the GPU). DXVK
can be enabled with ARCADE_SUPERCARS_DXVK=1, but it does not stretch the
game's 1360x768 2D menus to the backbuffer (they appear in a corner).

The game always runs with -res13 (1360x768). With the operator setting
Stretch Screen it lays out each frame in 1360x768 and Present stretches that
area to the screen-sized backbuffer; other -res sizes enlarge the intro and
races but not that area, so they appear zoomed in. The resolution setting
sizes the Wine virtual desktop in windowed play.

Environment (set by the Rust launcher): ARCADE_SUPERCARS_KEYS (controls.cfg),
ARCADE_RESOLUTION / ARCADE_GUEST_ENV (settings.cfg), ARCADE_AUDIO /
ARCADE_AUDIO_SINK. Optional: ARCADE_WINE (wine binary), ARCADE_SUPERCARS_DXVK=1
with ARCADE_DXVK (directory holding 32-bit d3d8.dll and d3d9.dll; found in
Steam's Proton installs otherwise), ARCADE_SUPERCARS_DEBUG=1 (log key events).
"""
import argparse
import fcntl
import glob
import hashlib
import os
from pathlib import Path
import shutil
import subprocess

from arcade_settings import resolution

# Below the games root (newest layout first), or the game folder itself.
GAME_DIRS = ['The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars',
             'windows based/The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars',
             'The_Fast_and_the_Furious_SuperCars', '.']
GAME_SHA256 = '0cddbdbf1d107d27dc668897d7851b6ac96b4349320551d800504b1c50b195ba'
# Window sizes offered for windowed play (the Wine virtual desktop).
RESOLUTIONS = ('640x480', '800x600', '1024x768', '1280x1024', '1360x768', '1600x1200', '1920x1080')
# The game's own option for 1360x768, the size it lays out and stretches (Stretch Screen).
RENDER_OPTION = '-res13'
# Not needed on the desktop: the community loader, its hooks and configs.
SKIP = {'MohkerzLoaderz.exe', 'MohkerzLoaderz.ini', 'MinHook.x86.dll', 'teknoparrot.ini',
        'dxwnd.dxw', 'errorlog.txt', 'messages.txt', 'dinput8.dll'}
# Small files the game rewrites (settings, audits, high scores, logs) are copied.
COPY_LIMIT = 1 << 20


def guest_settings():
    env = {}
    for line in os.environ.get('ARCADE_GUEST_ENV', '').splitlines():
        key, sep, value = line.partition('=')
        if sep:
            env[key] = value
    return env


def find_dxvk():
    candidates = [os.environ.get('ARCADE_DXVK', '')]
    steam = [Path.home() / '.local/share/Steam', Path.home() / '.steam/steam']
    for base in steam:
        for pattern in ['steamapps/common/Proton*/files/lib/wine/dxvk/i386-windows',
                        'compatibilitytools.d/*/files/lib/wine/dxvk/i386-windows']:
            candidates += sorted(glob.glob(str(base / pattern)), reverse=True)
    candidates += ['/usr/share/dxvk/x32', '/usr/lib32/dxvk']
    for c in candidates:
        if c and all((Path(c) / f).is_file() for f in ('d3d8.dll', 'd3d9.dll')):
            return Path(c)
    return None


def map_game(game, rawart):
    rawart.mkdir(parents=True, exist_ok=True)
    for src in sorted(game.iterdir()):
        if src.name in SKIP:
            continue
        dst = rawart / src.name
        small = src.is_file() and src.stat().st_size < COPY_LIMIT and src.suffix.lower() not in ('.exe', '.dll')
        if small:
            if not dst.exists() or dst.is_symlink():
                dst.unlink(missing_ok=True)
                shutil.copy2(src, dst)
        elif not (dst.is_symlink() and dst.readlink() == src):
            if dst.is_dir() and not dst.is_symlink():
                raise SystemExit(f'Unexpected directory in the prefix: {dst}')
            dst.unlink(missing_ok=True)
            dst.symlink_to(src)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--games-root', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    game = next((args.games_root / d for d in GAME_DIRS if (args.games_root / d / 'sdaemon.exe').is_file()),
                args.games_root / GAME_DIRS[0]).resolve()
    exe = game / 'sdaemon.exe'
    if not exe.is_file() or hashlib.sha256(exe.read_bytes()).hexdigest() != GAME_SHA256:
        raise SystemExit('Unsupported or missing Super Cars sdaemon.exe build')
    hook = root / 'target/i686-pc-windows-msvc/release/arcade_wine_hook.dll'
    if not hook.is_file():
        raise SystemExit('Build the input adapter first: cargo build -p banana-super-cars-hook '
                         '--target i686-pc-windows-msvc --release')
    wine = os.environ.get('ARCADE_WINE') or shutil.which('wine')
    if not wine:
        raise SystemExit('Wine is required (install wine, or set ARCADE_WINE)')
    settings = guest_settings()
    software = settings.get('LIBGL_ALWAYS_SOFTWARE') == '1'
    dxvk = None
    if os.environ.get('ARCADE_SUPERCARS_DXVK') == '1' and not software:
        dxvk = find_dxvk()
        if not dxvk:
            raise SystemExit('DXVK 32-bit d3d8/d3d9 not found (install Proton in Steam, or set ARCADE_DXVK)')
    res = resolution() if 'ARCADE_RESOLUTION' in os.environ else '1920x1080'
    if res not in RESOLUTIONS:
        raise SystemExit(f'Super Cars cannot use {res}; choose one of {", ".join(RESOLUTIONS)}')
    fullscreen = settings.get('ARCADE_FULLSCREEN') == '1'

    output = root / 'runtime/supercars'
    output.mkdir(parents=True, exist_ok=True)
    prefix = output / 'prefix'
    with (output / 'session.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit('A Super Cars session is already running')
        env = dict(os.environ, WINEPREFIX=str(prefix), WINEDEBUG='-all', WINEARCH='win64')
        env.pop('ARCADE_GUEST_ENV', None)
        log = (output / 'game.log').open('w')
        if not (prefix / 'system.reg').exists():
            print('[supercars] creating Wine prefix (first launch only)', flush=True)
            subprocess.run([wine, 'wineboot', '-i'], env=dict(env, WINEDLLOVERRIDES='mscoree,mshtml='),
                           stdout=log, stderr=log, check=True)
            server = Path(wine).with_name('wineserver')
            subprocess.run([str(server) if server.exists() else 'wineserver', '-w'], env=env)
        # The game only needs C:. Dropping Z: keeps the rest of the filesystem out of view.
        z = prefix / 'dosdevices/z:'
        if z.is_symlink():
            z.unlink()
        rawart = prefix / 'drive_c/rawart'
        map_game(game, rawart)
        shutil.copyfile(hook, rawart / 'dinput8.dll')
        syswow = prefix / 'drive_c/windows/syswow64'
        if dxvk:
            for name in ('d3d8.dll', 'd3d9.dll'):
                shutil.copyfile(dxvk / name, syswow / name)
        overrides = ['dinput8=n,b', 'mscoree,mshtml=']
        overrides.append('d3d8,d3d9=n,b' if dxvk else 'd3d8,d3d9=b')
        if os.environ.get('ARCADE_AUDIO', '1') == '0':
            overrides.append('winepulse.drv,winealsa.drv=')
        env['WINEDLLOVERRIDES'] = ';'.join(overrides)
        sink = os.environ.get('ARCADE_AUDIO_SINK')
        if sink:
            env['PULSE_SINK'] = sink
        # GL driver choices from settings.cfg (vsync, Mesa software rendering).
        env.update({k: v for k, v in settings.items() if not k.startswith('ARCADE_')})
        vsync = settings.get('ARCADE_VSYNC')
        if vsync is not None and dxvk:
            env['DXVK_CONFIG'] = f'd3d9.presentInterval = {1 if vsync == "1" else 0}'
        env['DXVK_LOG_PATH'] = str(output)
        env['DXVK_LOG_LEVEL'] = 'warn'
        # Windowed play uses a Wine virtual desktop of the chosen size; the game
        # always requests an exclusive fullscreen D3D device.
        key = r'HKCU\Software\Wine\Explorer'
        if fullscreen:
            subprocess.run([wine, 'reg', 'delete', key, '/v', 'Desktop', '/f'], env=env, stdout=log, stderr=log)
        else:
            subprocess.run([wine, 'reg', 'add', key, '/v', 'Desktop', '/d', 'SuperCars', '/f'],
                           env=env, stdout=log, stderr=log, check=True)
            subprocess.run([wine, 'reg', 'add', key + r'\Desktops', '/v', 'SuperCars', '/d', res, '/f'],
                           env=env, stdout=log, stderr=log, check=True)
        (rawart / 'arcade-hook.log').unlink(missing_ok=True)
        print(f'[supercars] starting at 1360x768, {"fullscreen" if fullscreen else "windowed " + res}, '
              f'{"DXVK " + str(dxvk) if dxvk else "WineD3D (software)" if software else "WineD3D (GPU)"}', flush=True)
        log.flush()
        status = subprocess.run([wine, 'sdaemon.exe', '-noerr', RENDER_OPTION], cwd=rawart, env=env,
                                stdout=log, stderr=log).returncode
        hook_log = rawart / 'arcade-hook.log'
        if hook_log.exists():
            shutil.copyfile(hook_log, output / 'arcade-hook.log')
        print(f'[supercars] game exited with status {status}; logs in {output}', flush=True)


if __name__ == '__main__':
    main()
