# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Set the centered title bar text in a rendered SVG.

Post-processes an already-rendered SVG from record.sh: fills in the
`<text id="chrome-title">` placeholder that _base_window_frame.svg leaves
empty, mimicking the "user@host - shell" text macOS puts in its own
terminal window titles (e.g. "user@host - zsh"). Left out of the base
template itself and out of build_themes.py's palette-driven generation on
purpose - this is personal/identity text, not theme data, so it's kept as
its own opt-in step instead of something baked into the (publicly
committed) base template or the (theme-only) palette pipeline.

Has nothing to set the text on if chrome has already been stripped
(strip_chrome.py) - run this first if combining the two.

Usage:
    uv run themes/set_window_title.py banner-kanagawa_wave.svg --title "user@host - zsh"
"""

import argparse
import re
from pathlib import Path
from xml.sax.saxutils import escape

# termtosvg's render step round-trips the template through an XML
# serializer, which collapses the placeholder from its base-template form
# (<text id="chrome-title" ...></text>, empty but with separate open/close
# tags) down to a self-closing <text id="chrome-title" .../> - so this has
# to match either form, and always rewrites to the open/close form since a
# self-closing tag has nowhere to put the title text.
TITLE_TAG_RE = re.compile(r'<text id="chrome-title"([^>]*?)(?:/>|>[^<]*</text>)')


def set_title(svg_text, title):
    new_text, count = TITLE_TAG_RE.subn(
        lambda m: f'<text id="chrome-title"{m.group(1)}>{escape(title)}</text>',
        svg_text,
        count=1,
    )
    if count == 0:
        raise RuntimeError(
            'Could not find a <text id="chrome-title"> placeholder - has chrome '
            "already been stripped (strip_chrome.py)?"
        )
    return new_text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--title", required=True)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args()

    result = set_title(args.input.read_text(), args.title)

    out_path = args.out or args.input.with_name(args.input.stem + "-titled.svg")
    out_path.write_text(result)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
