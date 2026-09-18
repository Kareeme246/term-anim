# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Detect the running terminal's live color scheme via OSC queries.

Writes OSC 10/11/12 (foreground/background/cursor) and OSC 4;N (the 16 ANSI
slots) queries directly to the controlling tty and reads the terminal's own
replies - the same mechanism tools like Neovim's background auto-detection
use. This works in most modern terminals (iTerm2, WezTerm, Kitty, Alacritty,
...) but NOT in Apple's stock Terminal.app, which doesn't answer OSC color
queries at all.

Must be run from a real terminal (a controlling TTY), same as record.sh -
it puts the tty in raw mode to read the replies, which requires stdin to be
an actual terminal device.

Output: writes themes/palettes/live.json in the same "Windows Terminal"
palette schema every other palette in this project uses, so it drops
straight into build_themes.py with no other changes - rerun that script
afterwards to generate live.svg / live_once.svg.

Usage:
    uv run themes/detect_terminal_theme.py
"""

import json
import os
import re
import select
import sys
import termios
import tty
from pathlib import Path

HERE = Path(__file__).parent
OUT_PATH = HERE / "palettes" / "live.json"

TIMEOUT = 0.35

# OSC code -> semantic name. 10/11/12 are fg/bg/cursor; 4;N is ANSI slot N.
ANSI_ORDER = [
    "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
    "brightPurple", "brightCyan", "brightWhite",
]

REPLY_RE = re.compile(
    rb"\x1b\](\d+);rgb:([0-9a-fA-F]+)/([0-9a-fA-F]+)/([0-9a-fA-F]+)(?:\x07|\x1b\\)"
)
# OSC 4 replies are prefixed with the color index too: "4;N;rgb:...".
REPLY4_RE = re.compile(
    rb"\x1b\]4;(\d+);rgb:([0-9a-fA-F]+)/([0-9a-fA-F]+)/([0-9a-fA-F]+)(?:\x07|\x1b\\)"
)


def component_to_8bit(hexstr):
    value = int(hexstr, 16)
    maxval = 16 ** len(hexstr) - 1
    return round(value * 255 / maxval)


def query(fd, message, index=None):
    os.write(fd, message)
    buf = b""
    pattern = REPLY4_RE if index is not None else REPLY_RE
    while True:
        ready, _, _ = select.select([fd], [], [], TIMEOUT)
        if not ready:
            return None
        chunk = os.read(fd, 256)
        if not chunk:
            return None
        buf += chunk
        match = pattern.search(buf)
        if match:
            if index is not None:
                r, g, b = match.group(2), match.group(3), match.group(4)
            else:
                r, g, b = match.group(2), match.group(3), match.group(4)
            return "#{:02x}{:02x}{:02x}".format(
                component_to_8bit(r.decode()),
                component_to_8bit(g.decode()),
                component_to_8bit(b.decode()),
            )


def main():
    if not sys.stdin.isatty():
        sys.exit(
            "stdin is not a real terminal - run this directly in your "
            "terminal (not piped, not from a sandboxed/agent shell)."
        )

    fd = sys.stdin.fileno()
    old_settings = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)

        background = query(fd, b"\x1b]11;?\x07")
        foreground = query(fd, b"\x1b]10;?\x07")
        cursor = query(fd, b"\x1b]12;?\x07")

        colors = {}
        for i, name in enumerate(ANSI_ORDER):
            colors[name] = query(fd, f"\x1b]4;{i};?\x07".encode(), index=i)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)

    missing = [k for k, v in {"background": background, "foreground": foreground, **colors}.items() if v is None]
    if background is None and foreground is None and not any(colors.values()):
        term_program = os.environ.get("TERM_PROGRAM", "unknown")
        sys.exit(
            f"No reply from the terminal to any OSC color query "
            f"(TERM_PROGRAM={term_program}). This terminal likely doesn't "
            f"support live color queries - Apple's Terminal.app is a known "
            f"case. Try iTerm2, WezTerm, Kitty, or Alacritty instead, or "
            f"build a palette by hand in themes/palettes/."
        )
    if missing:
        print(f"warning: no reply for: {', '.join(missing)} (left unset)", file=sys.stderr)

    palette = {
        "name": f"Live ({os.environ.get('TERM_PROGRAM', 'terminal')})",
        "background": background or "#000000",
        "foreground": foreground or "#ffffff",
        "cursorColor": cursor or foreground or "#ffffff",
        "selectionBackground": foreground or "#ffffff",
    }
    palette.update({k: v for k, v in colors.items() if v is not None})
    # Fill any color the terminal didn't answer for with a safe fallback.
    for name in ANSI_ORDER:
        palette.setdefault(name, "#808080")

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(palette, indent=2) + "\n")
    print(f"wrote {OUT_PATH}")
    print(f"background={palette['background']} foreground={palette['foreground']}")
    print("run: uv run themes/build_themes.py   to generate live.svg / live_once.svg")


if __name__ == "__main__":
    main()
