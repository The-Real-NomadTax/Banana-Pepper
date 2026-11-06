#!/usr/bin/env python3
"""Virtual cabinet joysticks for Minecraft Dungeons Arcade.

Creates four uinput gamepads (needs write access to /dev/uinput, e.g. the
`input` group). The game reads them through its built-in SDL/udev joystick code.
With --keyboard, keys from the host keyboard drive the pads while a window named
"Minecraft" has focus. Keys are read without grabbing, so other apps keep working.
"""
import argparse
import selectors
import subprocess
import time

import evdev
from evdev import UInput, AbsInfo, ecodes as e

BUTTONS = [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR,
           e.BTN_SELECT, e.BTN_START, e.BTN_THUMBL, e.BTN_THUMBR]
AXES = [(e.ABS_X, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_Y, AbsInfo(0, -32768, 32767, 16, 128, 0)),
        (e.ABS_HAT0X, AbsInfo(0, -1, 1, 0, 0, 0)), (e.ABS_HAT0Y, AbsInfo(0, -1, 1, 0, 0, 0))]

# key -> (player, kind, code, value)
KEYMAP = {
    # Player 1: arrows move, Z/X/C/V buttons, 1 start, 5 select
    e.KEY_LEFT: (0, "axis", e.ABS_X, -32767), e.KEY_RIGHT: (0, "axis", e.ABS_X, 32767),
    e.KEY_UP: (0, "axis", e.ABS_Y, -32767), e.KEY_DOWN: (0, "axis", e.ABS_Y, 32767),
    e.KEY_Z: (0, "btn", e.BTN_SOUTH, 1), e.KEY_X: (0, "btn", e.BTN_EAST, 1),
    e.KEY_C: (0, "btn", e.BTN_WEST, 1), e.KEY_V: (0, "btn", e.BTN_NORTH, 1),
    e.KEY_1: (0, "btn", e.BTN_START, 1), e.KEY_5: (0, "btn", e.BTN_SELECT, 1),
    # Player 2: IJKL move, U/O/P/; buttons, 2 start
    e.KEY_J: (1, "axis", e.ABS_X, -32767), e.KEY_L: (1, "axis", e.ABS_X, 32767),
    e.KEY_I: (1, "axis", e.ABS_Y, -32767), e.KEY_K: (1, "axis", e.ABS_Y, 32767),
    e.KEY_U: (1, "btn", e.BTN_SOUTH, 1), e.KEY_O: (1, "btn", e.BTN_EAST, 1),
    e.KEY_P: (1, "btn", e.BTN_WEST, 1), e.KEY_SEMICOLON: (1, "btn", e.BTN_NORTH, 1),
    e.KEY_2: (1, "btn", e.BTN_START, 1), e.KEY_6: (1, "btn", e.BTN_SELECT, 1),
}

# Host evdev keys -> SDL2 scancodes consumed by the native desktop adapter.
NATIVE_KEYS = dict(zip(
    [e.KEY_1, e.KEY_2, e.KEY_3, e.KEY_4, e.KEY_5, e.KEY_6, e.KEY_7, e.KEY_8,
     e.KEY_Z, e.KEY_X, e.KEY_C, e.KEY_V, e.KEY_LEFT, e.KEY_RIGHT, e.KEY_UP, e.KEY_DOWN,
     e.KEY_I, e.KEY_J, e.KEY_K, e.KEY_L, e.KEY_U, e.KEY_O, e.KEY_P, e.KEY_SEMICOLON,
     e.KEY_F1, e.KEY_F2, e.KEY_F3, e.KEY_F4],
    [30, 31, 32, 33, 34, 35, 36, 37, 29, 27, 6, 25, 80, 79, 82, 81,
     12, 13, 14, 15, 24, 18, 19, 51, 58, 59, 60, 61]))
for key, source in [(e.KEY_ENTER, 40), (e.KEY_SPACE, 44),
                    (e.KEY_KP1, 89), (e.KEY_KP2, 90), (e.KEY_KP3, 91), (e.KEY_KP4, 92),
                    (e.KEY_KP5, 93), (e.KEY_KP6, 94), (e.KEY_KP7, 95), (e.KEY_KP8, 96)]:
    NATIVE_KEYS[key] = source


def make_pads(count):
    caps = {e.EV_KEY: BUTTONS, e.EV_ABS: AXES}
    return [UInput(caps, name=f"Arcade Foundry Pad {i + 1}", vendor=0x045e, product=0x028e,
                   version=0x110 + i, bustype=e.BUS_USB) for i in range(count)]


def focused():
    try:
        name = subprocess.run(["xdotool", "getwindowfocus", "getwindowname"], capture_output=True,
                              text=True, timeout=1).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return False
    # The game window, or the 60 Hz gamescope window it runs inside.
    return name in ("Minecraft", "gamescope")


def keyboards():
    result = []
    for path in evdev.list_devices():
        device = evdev.InputDevice(path)
        keys = device.capabilities().get(e.EV_KEY, [])
        if e.KEY_Z in keys and e.KEY_1 in keys and not device.name.startswith("Arcade Foundry"):
            result.append(device)
    return result


def run(pads, keyboard, seconds=None, stop=None, scan=None, native_key=None):
    deadline = time.monotonic() + seconds if seconds else None
    selector = selectors.DefaultSelector()
    if keyboard:
        for device in keyboards():
            selector.register(device, selectors.EVENT_READ)
        print("Keyboard mapping active while the Minecraft window is focused", flush=True)
    held = {}
    native_held = set()
    last_focus_check, has_focus = 0.0, False
    try:
        while (deadline is None or time.monotonic() < deadline) and not (stop and stop.is_set()):
            events = selector.select(timeout=0.25) if selector.get_map() else (time.sleep(0.25) or [])
            now = time.monotonic()
            if now - last_focus_check > 0.3:
                last_focus_check, has_focus = now, focused()
                if not has_focus and native_key:
                    for source in native_held:
                        native_key(source, 0)
                    native_held.clear()
                if not has_focus and held:
                    for player, kind, code, _ in held.values():
                        pads[player].write(e.EV_ABS if kind == "axis" else e.EV_KEY, code, 0)
                        pads[player].syn()
                    held.clear()
            for key, _ in events:
                for event in key.fileobj.read():
                    if native_key:
                        source = NATIVE_KEYS.get(event.code)
                        if (event.type == e.EV_KEY and source is not None and event.value != 2
                                and (has_focus or source in native_held)):
                            native_key(source, event.value)
                            if event.value:
                                native_held.add(source)
                            else:
                                native_held.discard(source)
                        continue
                    if (scan and event.type == e.EV_KEY and event.value == 1 and has_focus
                            and e.KEY_F1 <= event.code <= e.KEY_F4):
                        scan(event.code - e.KEY_F1 + 1)
                        continue
                    if event.type != e.EV_KEY or event.code not in KEYMAP or event.value == 2:
                        continue
                    player, kind, code, value = KEYMAP[event.code]
                    if player >= len(pads) or (event.value and not has_focus):
                        continue
                    if event.value:
                        held[event.code] = KEYMAP[event.code]
                    else:
                        held.pop(event.code, None)
                    out = value if event.value else 0
                    pads[player].write(e.EV_ABS if kind == "axis" else e.EV_KEY, code, out)
                    pads[player].syn()
    except KeyboardInterrupt:
        pass
    finally:
        if native_key:
            for source in native_held:
                native_key(source, 0)
        for key in list(selector.get_map().values()):
            key.fileobj.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--count", type=int, default=4)
    parser.add_argument("--keyboard", action="store_true")
    parser.add_argument("--seconds", type=float, help="exit after N seconds")
    args = parser.parse_args()
    pads = make_pads(args.count)
    print(f"Created {len(pads)} virtual joysticks", flush=True)
    try:
        run(pads, args.keyboard, args.seconds)
    finally:
        for pad in pads:
            pad.close()


if __name__ == "__main__":
    main()
