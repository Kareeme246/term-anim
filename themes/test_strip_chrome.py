# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Self-check for themes/strip_chrome.py.

Run with: uv run themes/test_strip_chrome.py

Guards the bits of strip_chrome that are easy to break silently: the screen
must slide to the origin and the outer canvas crop to match, the chrome
elements must all go, and #terminalui must survive (it's the only opaque
background left once the screen's own .background is transparent - see
build_themes.py) with its corner rounding removed.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from strip_chrome import strip_chrome  # noqa: E402

# Trimmed to the elements strip_chrome actually looks for, in the same order
# the base template emits them.
RENDERED = """<svg xmlns="http://www.w3.org/2000/svg" id="terminal" viewBox="0 0 20 38" width="20">
    <defs><style type="text/css">.background {fill: none}</style></defs>
    <rect id="terminalui" width="100%" height="100%" ry="2"/>
    <circle cx="3" cy="3" r="1" class="color1"/>
    <text id="chrome-title" x="50%" y="18" class="chrome-muted">t</text>
    <rect id="chrome-divider" class="chrome-muted" x="0" y="27" width="100%" height="1"/>
    <svg id="screen" width="20" height="10" x="0" y="28" viewBox="0 0 20 10" preserveAspectRatio="xMidYMin slice"><g><g><rect/></g></g></svg>
</svg>
"""


def test():
    out = strip_chrome(RENDERED)

    # Chrome elements all gone.
    for gone in ("<circle", "chrome-title", "chrome-divider"):
        assert gone not in out, f"{gone} survived"

    # #terminalui survives as the frame's opaque background, minus rounding.
    assert '<rect id="terminalui"' in out, "#terminalui was removed"
    assert 'ry=' not in out.split('id="terminalui"', 1)[1].split("/>", 1)[0], (
        "#terminalui kept its corner rounding"
    )

    # Screen slid to the origin, outer canvas cropped to the screen height.
    assert (
        '<svg id="screen" width="20" height="10" x="0" y="0" viewBox="0 0 20 10"'
        in out
    ), "screen not repositioned to origin"
    assert 'viewBox="0 0 20 10" width="20"' in out, "outer canvas not cropped"
    assert out.count("</svg>") == 2, "expected exactly the screen + outer closes"
    assert out.rstrip().endswith("</svg>")

    print("strip_chrome: OK")


if __name__ == "__main__":
    test()
