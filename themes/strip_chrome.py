# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Remove the macOS-style title bar from an already-rendered SVG.

record.sh always renders through the "window frame" template (see
_base_window_frame.svg): a title-bar strip up top (three traffic-light
dots, a centered title, and a divider line) sitting directly above the
terminal screen, which itself is flush against the left/right/bottom
edges already. This strips that strip off entirely: removes the circles,
title text, and divider, squares off the window background's rounded
corners, then slides the screen up to the origin and crops the canvas down
to match it - so the terminal output becomes the whole frame, with no
padding on any side. Doesn't touch colors, text, scale, or animation; the
screen's own width/height/viewBox are untouched, so nothing stretches or
distorts - it's purely a reposition + crop.

Usage:
    uv run themes/strip_chrome.py banner-kanagawa_wave.svg
"""

import argparse
import re
from pathlib import Path

CIRCLE_RE = re.compile(r'\s*<circle[^>]*class="color[123]"[^>]*/>')
# Matches either form the placeholder can be in after a render - see
# set_window_title.py for why the open/close form can end up self-closed.
TITLE_TEXT_RE = re.compile(r'\s*<text id="chrome-title"[^>]*?(?:/>|>[^<]*</text>)')
DIVIDER_RE = re.compile(r'\s*<rect id="chrome-divider"[^>]*/>')
# #terminalui is the window background sitting behind the (transparent-bg)
# screen - see build_themes.py. Chromeless mode keeps it as the one opaque
# background for the frame, but drops its corner rounding so the terminal
# fills the whole canvas with square, flush edges and no transparent nicks.
TERMINALUI_ROUNDING_RE = re.compile(r'(<rect id="terminalui"[^>]*?)\s+ry="\d+"')
SCREEN_TAG_RE = re.compile(
    r'<svg id="screen" width="(\d+)" height="(\d+)" x="\d+" y="\d+" '
    r'viewBox="0 0 \d+ \d+"'
)
OUTER_VIEWBOX_RE = re.compile(r'viewBox="0 0 (\d+) \d+" width="\d+"')


def strip_chrome(svg_text):
    screen_match = SCREEN_TAG_RE.search(svg_text)
    if not screen_match:
        raise RuntimeError('Could not find the <svg id="screen"> element')
    screen_w, screen_h = screen_match.group(1), screen_match.group(2)

    svg_text = CIRCLE_RE.sub("", svg_text)
    svg_text = TITLE_TEXT_RE.sub("", svg_text)
    svg_text = DIVIDER_RE.sub("", svg_text)
    svg_text = TERMINALUI_ROUNDING_RE.sub(r"\1", svg_text)

    svg_text = SCREEN_TAG_RE.sub(
        f'<svg id="screen" width="{screen_w}" height="{screen_h}" x="0" y="0" '
        f'viewBox="0 0 {screen_w} {screen_h}"',
        svg_text,
        count=1,
    )

    outer_match = OUTER_VIEWBOX_RE.search(svg_text)
    if not outer_match:
        raise RuntimeError("Could not find the outer <svg> viewBox/width")
    outer_w = outer_match.group(1)
    svg_text = OUTER_VIEWBOX_RE.sub(
        f'viewBox="0 0 {outer_w} {screen_h}" width="{outer_w}"',
        svg_text,
        count=1,
    )

    return svg_text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args()

    result = strip_chrome(args.input.read_text())

    out_path = args.out or args.input.with_name(args.input.stem + "-chromeless.svg")
    out_path.write_text(result)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
