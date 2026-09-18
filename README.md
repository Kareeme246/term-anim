# term-anim

Create animated SVGs of scripted terminal sessions with a native desktop app. Commands are displayed, not executed, so demos are safe and repeatable.

Built on termtosvg, with themes from iTerm2-Color-Schemes.

## Demo

> Demo video coming soon.

## Quick start

term-anim is currently source-only. A downloadable app will replace these build steps later.

### Prerequisites

- [Git](https://git-scm.com/downloads)
- [`uv`](https://docs.astral.sh/uv/getting-started/installation/)
- [Rust](https://rustup.rs/)
- A real terminal window for recording

### Download and run

```sh
git clone https://github.com/Kareeme246/term-anim.git
cd term-anim/gui
cargo run
```

Add your commands and output as turns, choose the appearance, then click **Record**. When recording finishes, open the SVG in a browser or reveal it in your file manager.

## CLI

If you do not want to use the app, run the recorder directly from the project root:

```sh
./record.sh [theme] [speed] [loop-delay-ms]
```

- `theme` selects a bundled theme and defaults to `kanagawa_wave`. Add `_once` to a theme name to stop on the final frame instead of looping.
- `speed` is the typing-speed multiplier and defaults to `1.0`; `2.0` is twice as fast and `0.5` is twice as slow.
- `loop-delay-ms` is the pause before the animation repeats and defaults to `5000`.

The command writes `banner-<theme>.svg` and an intermediate `demo.cast` file.

### Preview a script

```sh
uv run banner.py [script.json] [--speed MULTIPLIER]
```

The optional path selects a script other than the default `script.json`. `--speed` uses the same typing-speed multiplier as `record.sh`.

### Optional SVG changes

Run these against an SVG produced by `record.sh`. Each accepts `--out <path>`; otherwise it writes a new file beside the input.

```sh
# Remove the title bar and window chrome
uv run themes/strip_chrome.py banner-kanagawa_wave.svg [--out PATH]

# Set the window title
uv run themes/set_window_title.py banner-kanagawa_wave.svg --title "user@host - zsh" [--out PATH]

# Set terminal opacity from 0.0 to 1.0
uv run themes/set_terminal_opacity.py banner-kanagawa_wave.svg --opacity 0.85 [--out PATH]

# Add a solid or two-color background
uv run themes/add_background.py banner-kanagawa_wave.svg [--bg HEX] [--bg-to HEX] [--angle DEGREES] [--padding PX] [--no-shadow] [--out PATH]
```

If combining them, remove the chrome first. `--bg-to` enables a gradient, while `--angle` controls its direction.

## Known limitations

- Recording and live-theme detection require a real controlling TTY. They cannot run from CI, a plain pipe, or a sandboxed agent shell.
- The cursor is a static inverted-color block, not an animated blink. Play-once animations freeze with the same static cursor.
- Apple's Terminal.app does not support the color queries used by live-theme detection.
