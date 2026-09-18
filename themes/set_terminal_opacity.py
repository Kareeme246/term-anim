# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Make a rendered terminal SVG's window translucent.

Post-processes an already-rendered SVG from record.sh: adjusts the
fill-opacity of both the `.background` rule (the terminal screen's own
cell background) and the `#terminalui` rule (the title bar strip, if
chrome hasn't been stripped by strip_chrome.py) so the whole window
becomes uniformly see-through - the same "window transparency" effect
real terminal apps offer. Text stays fully opaque either way. Doesn't
touch colors, geometry, or animation.

Usage:
    uv run themes/set_terminal_opacity.py banner-kanagawa.svg --opacity 0.85
"""

import argparse
import re
from pathlib import Path

# Matches both `.background { ... }` and `#terminalui { ... }` - the two
# CSS rules a rendered SVG uses for its translucent regions (see
# build_themes.py's build_user_style). A chrome-stripped SVG (see
# strip_chrome.py) has no #terminalui rule left, which is fine - this
# just matches whichever of the two are present.
BACKGROUND_RULE_RE = re.compile(r"(\.background|#terminalui)\s*\{([^}]*)\}")


def apply_opacity(svg_text, opacity):
    def repl(match):
        selector = match.group(1)
        body = match.group(2).strip()
        if body and not body.endswith(";"):
            body += ";"
        return f"{selector} {{{body} fill-opacity: {opacity};}}"

    new_text, count = BACKGROUND_RULE_RE.subn(repl, svg_text)
    if count == 0:
        raise RuntimeError(
            "Could not find a `.background { ... }` or `#terminalui { ... }` "
            "rule to adjust"
        )
    return new_text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument(
        "--opacity", type=float, required=True,
        help="0.0 (fully transparent) to 1.0 (fully opaque)",
    )
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args()

    result = apply_opacity(args.input.read_text(), args.opacity)

    out_path = args.out or args.input.with_name(args.input.stem + "-opacity.svg")
    out_path.write_text(result)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
