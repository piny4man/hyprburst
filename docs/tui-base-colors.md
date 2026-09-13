# External TUI base colors

The external TUI adapter fills its widget area with the resolved foreground and
background before invoking the shared renderer. Ordinary text and blank cells
inherit those colors; selection, prompt, banner, and empty-state styles still
win. An explicit `Color::Reset` remains the terminal's corresponding default,
not an instruction to inherit the theme's base color. Foreground fade-in skips
Reset cells because their actual RGB value belongs to the terminal/GPU host.

Without a valid shared theme, config still resolves the built-in palette plus
explicit overrides. Applying that palette to the external TUI's base cells is
intentional; previously these cells used terminal defaults. To restore that
behavior, set both `[colors] foreground = "reset"` and `background = "reset"`.
Terminal-owned font and opacity settings remain independent.

## GPU isolation

The native frontend still calls `render_core` directly and receives Reset base
cells. Rio spawns its TUI child with the internal `tui --gpu-host` argument.
That mode changes only the child's base foreground/background to Reset; its
selection/accent colors remain explicit. The parent resolves ANSI defaults to
its own configured colors and controls the translucent/opaque background panel.
This avoids painting opaque base backgrounds into every Rio cell. The internal
argument is not inherited by applications launched from the child.

## Regression coverage

`cargo test --all` covers:

- Theme loading through an original fixture, explicit base overrides, independent
  foreground/background resets, ordinary app text, blank cells, and prompt colors
  using ratatui's `TestBackend`.
- Selected-row foreground/background (including Reset), bold styling, and padding.
- Built-in no-theme palette, widget-area bounds, and empty-result background.
- Unchanged native shared-buffer Reset cells, Rio child arguments, and host config
  adaptation preserving every field other than the two base colors.
- Reset foreground preservation throughout fade-in, alongside existing explicit
  foreground animation tests.

## Live smoke record — 2026-09-13

Environment: Hyprland 0.56.2, Wayland, eDP-1 at 1.5× display scaling.
All runs used isolated XDG config/data directories and two harmless desktop
entries, Alpha and Beta (`Exec=true`); neither was launched. The Swatches theme
used foreground `#EAF3FF`, background `#10253F`, and selection colors
`#FFFFFF` / `#244A70` (see `tests/fixtures/themes/base.toml`).

Actual `hyprburst tui` output was captured in a 40×10 PTY, allowed to settle past
fade-in, and parsed with pyte. Assertions checked Beta's unselected text, top-left
and bottom-right blank cells, and Alpha's selection colors. All cases passed:

| Case | Base foreground / background |
| --- | --- |
| Shared theme | `#EAF3FF` / `#10253F` |
| Theme with explicit overrides | `#123456` / `#654321` |
| No theme | `#C8CCE0` / `#1A1B26` |
| Explicit base resets | ANSI default / default |
| Rio internal GPU-host mode | ANSI default / default |

Each PTY run exited successfully on Escape. This caught the fade effect turning
Reset foregrounds into explicit white before the regression fix.

Visual checks also passed in three real windows:

- Kitty, started without user config and with deliberately different defaults
  (`foreground=#003311`, `background=#FFCCAA`): launcher text, selection, and blank
  cells used the shared theme instead. Kitty's outer padding remained terminal-owned.
- Native and Rio with `window.transparent=true`, `window.opacity=0.4`: matching
  themed text/selection and visible blurred wallpaper through the base background.
  No new opaque base-cell rectangles appeared in Rio.

All three window runs had empty stdout/stderr logs. Each was captured with grim
1.5 seconds after mapping, then its test process was terminated. No compositor
settings or user config were changed. Temporary scripts, PTY captures, logs, and
screenshots are in `/tmp/hyprburst-tui-smoke.2yuDxf/` (`smoke.py`, `gui.py`,
`*.ansi`, `external.png`, `native.png`, `rio.png`); these are local artifacts, not
packaged assets. The PTY script runs via `uv run --with pyte python smoke.py`.
