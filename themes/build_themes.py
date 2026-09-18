# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Generate termtosvg templates from real terminal color-scheme palettes.

Palettes live in themes/palettes/*.json using the "Windows Terminal" scheme
schema (background, foreground, black..white, brightBlack..brightWhite) -
the same schema used by https://github.com/mbadolato/iTerm2-Color-Schemes,
which is the palette source WezTerm and most terminal emulators draw their
bundled theme lists from. Every palette JSON in themes/palettes/ is picked
up automatically (the filename stem becomes the theme slug) - to add a new
theme, just drop a palette JSON in that schema into themes/palettes/ and
rerun this script.

The window-frame chrome (rounded window, traffic-light buttons, geometry)
comes from termtosvg's own "window_frame" template, cached locally as
_base_window_frame.svg so this doesn't depend on termtosvg's install path.
Only the 16-color palette and the character grid (COLUMNS x ROWS) are
swapped in per theme.

Usage:
    uv run themes/build_themes.py            # rebuild every theme
    uv run themes/build_themes.py live        # rebuild just themes/palettes/live.json

Pass a single palette slug (its filename stem) to regenerate only that
theme, e.g. after detect_terminal_theme.py refreshes live.json - this
skips the wipe-and-rebuild-everything pass below, so it doesn't churn all
600+ other theme files for a one-palette change.
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).parent
BASE_TEMPLATE = HERE / "_base_window_frame.svg"
PALETTES_DIR = HERE / "palettes"
OUT_DIR = HERE

# Final character grid, shared by every generated theme so they're all
# interchangeable with the same recording (see record.sh).
COLUMNS = 92
ROWS = 24

# Cell size baked into termtosvg's window_frame template: 656/82 x 323/19.
CELL_WIDTH = 8
CELL_HEIGHT = 17
# The screen sits flush against the left/right/bottom edges of the frame -
# no padding there. The only fixed chrome is the title bar strip on top
# (traffic lights + a divider line), which is why this is the one margin
# left: it's a constant band height, not proportional to the character
# grid. themes/strip_chrome.py removes this band entirely for a
# chrome-free frame.
TITLEBAR_HEIGHT = 28

# Windows Terminal schema key -> ANSI color slot (0-15)
ANSI_ORDER = [
    "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
    "brightPurple", "brightCyan", "brightWhite",
]


# Appended to a theme's CSS to play the recording once and hold on the
# final frame instead of looping. Lifted straight from termtosvg's own
# "gjm8_single_loop" built-in template: the generated animation already sets
# animation-fill-mode:forwards, so overriding just the iteration count here
# freezes on the last frame rather than snapping back to a blank one.
FREEZE_CSS = "\n\n            #screen_view {\n                animation-iteration-count: 1;\n            }"


# Opacity applied to .chrome-muted (the title text + divider line) - low
# enough to read as a quiet hairline/caption rather than a bold element,
# regardless of which theme it's rendered in.
CHROME_MUTED_OPACITY = 0.45


def build_user_style(palette):
    lines = [
        f"            .foreground {{fill: {palette['foreground']}}}",
        # The terminal screen's own cell background is left transparent: the
        # window background is drawn once, by #terminalui (a rounded rect
        # behind everything), so the screen's flush opaque rect can't square
        # off the window's rounded bottom corners - no clip-path needed to
        # hide it (see _base_window_frame.svg). strip_chrome.py squares
        # #terminalui off for the chrome-free case, where it's the only
        # background left.
        f"            .background {{fill: none}}",
        f"            #terminalui {{fill: {palette['background']}}}",
        # termtosvg's generated stylesheet sets
        # `text {{ dominant-baseline: text-before-edge }}` for the terminal
        # grid, which also catches this title placeholder and makes its y
        # attribute behave as the text's top rather than its baseline -
        # pushing the title low in the bar and clipping it against the
        # screen below. Restore the normal baseline for it.
        f"            #chrome-title {{dominant-baseline: auto}}",
        # The divider line and title text: some themes' ANSI color8
        # ("bright black") is too saturated or too light/dark to read as
        # subtle chrome, so instead of picking a fixed color slot, this
        # dims the theme's own foreground color via opacity - always
        # legible against that theme's background, and always quiet.
        f"            .chrome-muted {{fill: {palette['foreground']}; "
        f"fill-opacity: {CHROME_MUTED_OPACITY}}}",
    ]
    for i, key in enumerate(ANSI_ORDER):
        lines.append(f"            .color{i} {{fill: {palette[key]}}}")
    return "\n".join(lines)


def build_theme(slug, palette_name, freeze=False):
    palette = json.loads((PALETTES_DIR / f"{palette_name}.json").read_text())
    svg = BASE_TEMPLATE.read_text()

    svg = svg.replace(
        'columns="82" rows="19"', f'columns="{COLUMNS}" rows="{ROWS}"'
    )

    screen_w = COLUMNS * CELL_WIDTH
    screen_h = ROWS * CELL_HEIGHT
    total_w = screen_w
    total_h = TITLEBAR_HEIGHT + screen_h

    svg = re.sub(
        r'viewBox="0 0 \d+ \d+" width="\d+"',
        f'viewBox="0 0 {total_w} {total_h}" width="{total_w}"',
        svg,
        count=1,
    )
    svg = re.sub(
        r'(<svg id="screen" width=")\d+(" height=")\d+('
        r'" x=")\d+(" y=")\d+(" viewBox="0 0 )\d+ \d+(")',
        rf'\g<1>{screen_w}\g<2>{screen_h}\g<3>0\g<4>{TITLEBAR_HEIGHT}'
        rf'\g<5>{screen_w} {screen_h}\g<6>',
        svg,
        count=1,
    )
    old_style_block = re.search(
        r'(<style type="text/css" id="user-style">\n).*?(\n\s*</style>)',
        svg,
        re.DOTALL,
    )
    if not old_style_block:
        raise RuntimeError("Could not find user-style block in base template")
    user_style = build_user_style(palette)
    if freeze:
        user_style += FREEZE_CSS
    svg = (
        svg[: old_style_block.start(1)]
        + old_style_block.group(1)
        + user_style
        + old_style_block.group(2)
        + svg[old_style_block.end(2):]
    )

    out_slug = f"{slug}_once" if freeze else slug
    out_path = OUT_DIR / f"{out_slug}.svg"
    out_path.write_text(svg)
    mode = "single-play, freezes on last frame" if freeze else "loops"
    print(f"wrote {out_path} ({palette['name']}, {COLUMNS}x{ROWS}, {mode})")


if __name__ == "__main__":
    only = sys.argv[1] if len(sys.argv) > 1 else None

    if only:
        if not (PALETTES_DIR / f"{only}.json").exists():
            raise SystemExit(
                f"No palette found at {PALETTES_DIR / f'{only}.json'}")
        build_theme(only, only, freeze=False)
        build_theme(only, only, freeze=True)
    else:
        palette_files = sorted(PALETTES_DIR.glob("*.json"))
        if not palette_files:
            raise SystemExit(f"No palette JSON files found in {PALETTES_DIR}")

        # Wipe previously generated templates first so a renamed/removed
        # palette can't leave a stale orphaned .svg behind - this directory
        # should only ever contain what the current palette set produces,
        # plus the base. Only done on a full rebuild, not a single-slug one.
        removed = 0
        for old in OUT_DIR.glob("*.svg"):
            if old.name != BASE_TEMPLATE.name:
                old.unlink()
                removed += 1
        if removed:
            print(f"removed {removed} stale generated template(s)")

        for path in palette_files:
            slug = path.stem
            build_theme(slug, slug, freeze=False)
            build_theme(slug, slug, freeze=True)
