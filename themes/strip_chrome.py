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
title text, and divider, slides the screen up to the origin, and crops the
canvas down to match - so the terminal output becomes the whole frame,
with no padding on any side. Doesn't touch colors, text, scale, or
animation; the screen's own width/height/viewBox are untouched, so nothing
stretches or distorts - it's purely a reposition + crop.

The frame keeps #terminalui's rounded corners (see _base_window_frame.svg),
and the screen is clipped to the same rounded shape: with the title bar
gone, the first line of terminal text sits right at the top edge, so
without the clip its glyphs and cursor would spill past the curves.

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
# screen - see build_themes.py. Chromeless mode keeps it, rounding and all,
# as the frame's background; the screen is clipped to the same rounded
# shape so the first line of text can't spill past the curves.
TERMINALUI_ROUNDING_RE = re.compile(
    r'<rect id="terminalui"[^>]*?\bry="(\d+)"'
)
SCREEN_TAG_RE = re.compile(
    r'<svg id="screen" width="(\d+)" height="(\d+)" x="\d+" y="\d+" '
    r'viewBox="0 0 \d+ \d+"'
)
OUTER_VIEWBOX_RE = re.compile(r'viewBox="0 0 (\d+) \d+" width="\d+"')
CLIP_ID = "term-anim-rounded"


def strip_chrome(svg_text):
    screen_match = SCREEN_TAG_RE.search(svg_text)
    if not screen_match:
        raise RuntimeError('Could not find the <svg id="screen"> element')
    screen_w, screen_h = screen_match.group(1), screen_match.group(2)

    svg_text = CIRCLE_RE.sub("", svg_text)
    svg_text = TITLE_TEXT_RE.sub("", svg_text)
    svg_text = DIVIDER_RE.sub("", svg_text)

    rounding = TERMINALUI_ROUNDING_RE.search(svg_text)
    if not rounding:
        raise RuntimeError('Could not find #terminalui\'s corner rounding')
    radius = rounding.group(1)
    clip_path = (
        f'<clipPath id="{CLIP_ID}">'
        f'<rect width="{screen_w}" height="{screen_h}" rx="{radius}" ry="{radius}"/>'
        f"</clipPath>"
    )
    svg_text = svg_text.replace("</defs>", clip_path + "</defs>", 1)

    svg_text = SCREEN_TAG_RE.sub(
        f'<svg id="screen" width="{screen_w}" height="{screen_h}" x="0" y="0" '
        f'viewBox="0 0 {screen_w} {screen_h}" clip-path="url(#{CLIP_ID})"',
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
