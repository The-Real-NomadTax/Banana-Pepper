#!/usr/bin/env python3
"""Send an eight-character QR card code to a running seeded Minecraft cabinet.

Example: python3 scripts/minecraft-cards.py a0000000 --player 1
The game performs its own card lookup/validation. Codes are case-sensitive.
"""
import argparse
import fcntl
import os
from pathlib import Path


def validate(player, code):
    if player not in range(1, 5):
        raise ValueError("Player must be 1-4")
    if len(code) != 8 or not code.isascii() or not code.isalnum():
        raise ValueError("Card code must contain exactly eight ASCII letters/digits")


def scan(output, player, code):
    validate(player, code)
    # Require an active cabinet; don't silently queue stale events for the next run.
    with (output / "session.lock").open("r") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            pass
        else:
            raise RuntimeError("Minecraft is not running")
        fd = os.open(output / "card-events", os.O_WRONLY | os.O_APPEND)
        try:
            os.write(fd, f"{player} {code}\n".encode("ascii"))
        finally:
            os.close(fd)
    print(f"Queued card {code} for player {player}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("code")
    parser.add_argument("--player", type=int, default=1)
    parser.add_argument("--runtime", type=Path, default=Path(__file__).resolve().parents[1] / "runtime/minecraft-seeded")
    args = parser.parse_args()
    try:
        scan(args.runtime, args.player, args.code)
    except (ValueError, RuntimeError, OSError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
