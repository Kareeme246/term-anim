# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Center a rendered terminal SVG on a background canvas.

Pads the terminal window out on all sides with a two-stop gradient) and drops
a soft shadow under it, so it reads as a floating card.
Works as a post-processing step on an already-rendered SVG (from
record.sh) - it doesn't touch colors or animation, just wraps the
existing SVG unmodified inside a bigger one.

The GUI curates a fixed set of ~10 preset gradients rather than exposing
a full color picker (a raw pick-any-solid-color backdrop reads flat and
cheap next to the terminal card - see term-anim-gui's BG_GRADIENTS); this
script itself stays generic, so any two colors work from the CLI too.

Usage:
    # solid color
    uv run themes/add_background.py banner-kanagawa.svg --bg "#1a1b26" --padding 40

    # two-stop gradient
    uv run themes/add_background.py banner-kanagawa.svg \\
        --bg "#1a1b26" --bg-to "#2d2b55" --angle 135 --padding 40
"""

import argparse
import re
from pathlib import Path

ROOT_TAG_RE = re.compile(
    r'<svg\b[^>]*\bviewBox="0 0 (?P<w>[\d.]+) (?P<h>[\d.]+)"[^>]*>', re.DOTALL
)


def frame(svg_text, bg, padding, shadow, bg_to=None, angle=135):
    match = ROOT_TAG_RE.search(svg_text)
    if not match:
        raise RuntimeError(
            "Could not find a viewBox on the input SVG's root element")
    inner_w = float(match.group("w"))
    inner_h = float(match.group("h"))

    total_w = inner_w + 2 * padding
    total_h = inner_h + 2 * padding

    # Nest the entire original SVG document as a child <svg>, positioned by
    # x/y - it keeps its own coordinate system, <defs>, <style> and CSS
    # animations exactly as rendered
    shadow_defs = ""
    shadow_attr = ""
    if shadow:
        shadow_defs = (
            '<filter id="term-anim-shadow" x="-50%" y="-50%" width="200%" height="200%">'
            '<feDropShadow dx="0" dy="12" stdDeviation="24" flood-color="#000000" flood-opacity="0.45"/>'
            "</filter>"
        )
        shadow_attr = ' filter="url(#term-anim-shadow)"'

    if bg_to:
        # gradientUnits defaults to objectBoundingBox, so a plain 0->1
        # left-to-right gradient plus a rotation around the box's center
        # is enough to angle it, without computing x1/y1/x2/y2.
        gradient_defs = (
            f'<linearGradient id="term-anim-bg-gradient" gradientTransform="rotate({angle:g} 0.5 0.5)">'
            f'<stop offset="0" stop-color="{bg}"/>'
            f'<stop offset="1" stop-color="{bg_to}"/>'
            f"</linearGradient>"
        )
        bg_fill = "url(#term-anim-bg-gradient)"
    else:
        gradient_defs = ""
        bg_fill = bg

    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{total_w:g}" height="{total_h:g}" '
        f'viewBox="0 0 {total_w:g} {total_h:g}">\n'
        f"  <defs>{gradient_defs}{shadow_defs}</defs>\n"
        f'  <rect width="100%" height="100%" fill="{bg_fill}"/>\n'
        f'  <g{shadow_attr}>\n'
        f'    <svg x="{padding:g}" y="{padding:g}" width="{inner_w:g}" height="{inner_h:g}">\n'
        f"{svg_text}\n"
        f"    </svg>\n"
        f"  </g>\n"
        f"</svg>\n"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="Rendered SVG from record.sh")
    parser.add_argument(
        "--bg", default="#1a1b26",
        help="Background color - solid fill by default, or the gradient's start "
        "color when --bg-to is given (default: #1a1b26)",
    )
    parser.add_argument(
        "--bg-to", default=None,
        help="Second gradient stop color - if given, this becomes a two-stop "
        "linear gradient instead of a solid fill",
    )
    parser.add_argument(
        "--angle", type=float, default=135,
        help="Gradient angle in degrees (default: 135, top-left to bottom-right); "
        "ignored without --bg-to",
    )
    parser.add_argument("--padding", type=float, default=40,
                        help="Padding in px on each side (default: 40)")
    parser.add_argument("--no-shadow", action="store_true",
                        help="Skip the drop shadow")
    parser.add_argument("--out", type=Path, default=None,
                        help="Output path (default: <input>-framed.svg)")
    args = parser.parse_args()

    svg_text = args.input.read_text()
    framed = frame(
        svg_text, args.bg, args.padding, shadow=not args.no_shadow,
        bg_to=args.bg_to, angle=args.angle,
    )

    out_path = args.out or args.input.with_name(
        args.input.stem + "-framed.svg")
    out_path.write_text(framed)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
