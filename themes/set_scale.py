# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Scale a rendered terminal SVG to any size without changing its layout.

Rewrites the outermost <svg>'s width/height to `scale` times its own
viewBox, leaving the viewBox and every inner coordinate, font metric and
keyframe untouched. SVG scaling is vectorial and lossless, so the result
is identical to the original, just bigger or smaller. The scale is
uniform, so the aspect ratio never changes - any factor still looks right.

Apply this last: it measures whatever the current outermost element is,
so it correctly scales a background frame added by add_background.py.

Usage:
    uv run themes/set_scale.py banner-kanagawa.svg --scale 2.0
"""

import argparse
import re
from pathlib import Path

# The outer <svg>, whatever it wraps. `width`/`height` are only matched as
# standalone attributes, so the `width` in `stroke-width` is left alone.
ROOT_TAG_RE = re.compile(
    r'<svg\b[^>]*\bviewBox="0 0 (?P<w>[\d.]+) (?P<h>[\d.]+)"[^>]*>', re.DOTALL
)
SIZE_ATTR_RE = re.compile(r'\s+(?<![-\w])(?:width|height)="[^"]*"')


def scale_svg(svg_text, scale):
    if scale <= 0:
        raise ValueError("scale must be positive")
    match = ROOT_TAG_RE.search(svg_text)
    if not match:
        raise RuntimeError("Could not find a viewBox on the input SVG's root element")
    width = float(match.group("w")) * scale
    height = float(match.group("h")) * scale
    head = re.sub(r"\s+", " ", SIZE_ATTR_RE.sub("", match.group(0))[:-1]).rstrip()
    new_tag = f'{head} width="{width:g}" height="{height:g}">'
    return svg_text[: match.start()] + new_tag + svg_text[match.end():]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="Rendered SVG from record.sh")
    parser.add_argument(
        "--scale", type=float, default=1.0,
        help="Uniform size multiplier, e.g. 2.0 for twice as large (default: 1.0)",
    )
    parser.add_argument("--out", type=Path, default=None,
                        help="Output path (default: <input>-scaled.svg)")
    args = parser.parse_args()

    out_path = args.out or args.input.with_name(args.input.stem + "-scaled.svg")
    out_path.write_text(scale_svg(args.input.read_text(), args.scale))
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
