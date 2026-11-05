"""Shared desktop-audio sandbox arguments for the Python launch harnesses.

Exposes only the user's PulseAudio / pipewire-pulse playback socket (and cookie,
when one exists) inside Bubblewrap, mirroring the Rust Big Buck Hunter launcher.
Set ARCADE_AUDIO=0 to force SDL's silent dummy backend. ARCADE_AUDIO_SINK
selects a specific Pulse sink.
"""
import os
from pathlib import Path


def pulse_socket():
    server = os.environ.get('PULSE_SERVER', '')
    path = server.removeprefix('unix:') if server.startswith(('unix:', '/')) else ''
    if path:
        return Path(path)
    runtime = os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}'
    return Path(runtime) / 'pulse/native'


def sdl_audio_args(driver):
    """Return (bwrap arguments, selected SDL audio driver name)."""
    if os.environ.get('ARCADE_AUDIO', '1') == '0':
        return ['--setenv', 'SDL_AUDIODRIVER', 'dummy'], 'dummy'
    socket = pulse_socket()
    if not socket.exists():
        print(f'[audio] no PulseAudio/PipeWire socket at {socket}; using silent dummy audio', flush=True)
        return ['--setenv', 'SDL_AUDIODRIVER', 'dummy'], 'dummy'
    args = ['--ro-bind', str(socket), '/tmp/arcade-pulse/native',
            '--setenv', 'PULSE_SERVER', 'unix:/tmp/arcade-pulse/native',
            '--setenv', 'SDL_AUDIODRIVER', driver]
    sink = os.environ.get('ARCADE_AUDIO_SINK')
    if sink:
        args += ['--setenv', 'PULSE_SINK', sink]
    cookie = Path(os.environ.get('PULSE_COOKIE', str(Path.home() / '.config/pulse/cookie')))
    if cookie.is_file():
        args += ['--ro-bind', str(cookie), '/tmp/arcade-pulse/cookie',
                 '--setenv', 'PULSE_COOKIE', '/tmp/arcade-pulse/cookie']
    return args, driver
