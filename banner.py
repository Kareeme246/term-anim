# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Fake-typed terminal session player.

Prints a scripted sequence of prompt/command/output turns to stdout, typing
each command out character by character. Meant to be captured by a terminal
recorder (e.g. termtosvg) to produce a banner animation - the "typing" is
fully deterministic and scripted, nothing is actually executed.

Usage:
    uv run banner.py [script.json] [--speed MULTIPLIER]
"""

import argparse
import json
import random
import sys
import time
from pathlib import Path

# ANSI SGR codes - termtosvg records these and maps them onto whichever
# theme's palette is active at render time, so the same codes look right
# across every generated theme.
BOLD_GREEN = "\x1b[1;32m"
BLUE = "\x1b[34m"
RESET = "\x1b[0m"

CHAR_DELAY = (0.05, 0.12)  # seconds, min/max per typed character
POST_COMMAND_PAUSE = 0.4  # pause after Enter, before output appears
POST_OUTPUT_PAUSE = 0.8  # pause after a turn's output, before next prompt
OUTRO_HOLD = 5.0  # seconds to sit idle on the closing prompt, giving readers time


def build_prompt(user_host):
    """`user@host:~$ ` if user_host is set, else just `:~$ ` - see script.json's
    top-level `user_host` field (optional; set from the GUI's Turns header)."""
    prefix = f"{BOLD_GREEN}{user_host}{RESET}" if user_host else ""
    return f"{prefix}:{BLUE}~{RESET}$ "


def type_out(text, speed):
    for ch in text:
        sys.stdout.write(ch)
        sys.stdout.flush()
        # Higher speed = faster typing (shorter delay) - a multiplier in the
        # everyday "2x speed" sense, not a delay scaler.
        time.sleep(random.uniform(*CHAR_DELAY) / speed)


def play(turns, prompt, speed=1.0):
    for turn in turns:
        sys.stdout.write(prompt)
        sys.stdout.flush()

        command = turn.get("command")
        if command:
            type_out(command, speed)
            sys.stdout.write("\n")
            sys.stdout.flush()
            time.sleep(POST_COMMAND_PAUSE / speed)

        output = turn.get("output", "")
        if output:
            sys.stdout.write(output)
            sys.stdout.flush()

        time.sleep(POST_OUTPUT_PAUSE / speed)

    # Outro: a second bare prompt, cursor left resting at the end. Not
    # scaled by speed - it's a fixed reading pause, not a typing delay.
    sys.stdout.write(prompt + "\n" + prompt)
    sys.stdout.flush()
    time.sleep(OUTRO_HOLD)


def load_script(path):
    data = json.loads(path.read_text())
    if isinstance(data, list):
        # Old script.json shape: a bare array of turns, no user_host field.
        return "", data
    return data.get("user_host", ""), data["turns"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Play a scripted fake-typed terminal session.")
    parser.add_argument(
        "script", nargs="?", type=Path, default=Path(__file__).parent / "script.json",
        help="Path to a script.json file (default: script.json next to this file)",
    )
    parser.add_argument(
        "--speed", type=float, default=1.0,
        help="Typing speed multiplier: 1.0 is default pace, 2.0 is twice as fast, 0.5 is twice as slow.",
    )
    args = parser.parse_args()
    user_host, turns = load_script(args.script)
    play(turns, build_prompt(user_host), speed=args.speed)
