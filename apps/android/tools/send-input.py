#!/usr/bin/env python3
"""Send afbridge commands through an existing adb forward.

Example: adb forward tcp:38461 localabstract:banana.probe.bigbuck
         send-input.py 38461 commands.txt
Commands match the Kotlin transport: m x y, b button pressed, k keysym pressed.
Lines beginning with @ pause for that many milliseconds.
"""
import argparse
import socket
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", type=int)
    parser.add_argument("commands", type=Path)
    args = parser.parse_args()
    with socket.create_connection(("127.0.0.1", args.port), timeout=5) as connection:
        for line in args.commands.read_text().splitlines():
            if not line.strip():
                continue
            if line.startswith("@"):
                time.sleep(max(0, min(int(line[1:]), 10000)) / 1000)
            elif line.startswith(("m ", "b ", "k ")):
                connection.sendall((line + "\n").encode("ascii"))
            else:
                raise ValueError(f"Invalid input command: {line}")


if __name__ == "__main__":
    main()
