"""Per-game settings handed over by the Rust launcher (settings.cfg).

ARCADE_RESOLUTION is the game's WIDTHxHEIGHT. ARCADE_GUEST_ENV holds
newline-separated KEY=VALUE pairs (fullscreen, vsync, renderer) that are set
inside the Bubblewrap sandbox. Both are absent for diagnostic runs, which keep
the cabinet defaults.
"""
import os
import re

DEFAULT_RESOLUTION = '1360x768'


def resolution():
    value = os.environ.get('ARCADE_RESOLUTION', DEFAULT_RESOLUTION)
    return value if re.fullmatch(r'[1-9][0-9]{2,3}x[1-9][0-9]{2,3}', value) else DEFAULT_RESOLUTION


def guest_env_args():
    """Bubblewrap --setenv arguments for the guest settings environment."""
    args = []
    for line in os.environ.get('ARCADE_GUEST_ENV', '').splitlines():
        key, sep, value = line.partition('=')
        if sep and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key):
            args += ['--setenv', key, value]
    return args
