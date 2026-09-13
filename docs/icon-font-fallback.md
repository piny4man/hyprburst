# GPU icon font fallback

Both the native and Rio frontends resolve an immutable font set at startup.
The first parseable font retains the existing precedence: explicit path,
`HYPRBURST_FONT`, configured/shared family, fontconfig candidates, hardcoded
paths. Only this primary font determines text-cell width, height, and baseline.
Remaining candidates supply missing glyphs; duplicate paths are loaded once.

Private-use icons use aspect-preserving ink-bound fitting and centering within
one primary text cell, regardless of the selected face's advance or line metrics.
When no face provides an icon, the renderer draws a font-independent four-tile
app badge. Text keeps its original scale and baseline. The external `tui`
continues to use its hosting terminal's font configuration.

The font set and rasterization size are fixed for a renderer's lifetime, so the
atlas remains keyed by the original character, not the fallback glyph ID. A
cache hit reuses the same tile without another rasterization/upload. Different
characters cannot collide merely because their font-local glyph IDs match.

## Deterministic regression coverage

Run `cargo test --all`. `src/gpu/font.rs` tests cover:

- A primary font missing the launcher's actual Firefox character U+F269 and a
  fallback containing it; supported primary text and icons still win.
- Visible, centered, unclipped icon coverage and atlas reuse at 1×, 1.5×, and 2×
  cell sizes, including oversized/negative-bearing and undersized outlines.
- Generic badges when no icon face exists, including supplementary private-use
  characters, and unchanged space/text rendering.
- First-parseable loading, duplicate paths, and path/environment/family ordering.

`tests/fixtures/fonts/*.ttf` are original geometric fixtures, not system fonts.
Their generator documents provenance and can be run with
`uv run --with fonttools python tests/fixtures/fonts/generate.py`.
Tests consume the checked-in fonts and do not require Python or fonttools.

## Live smoke record — 2026-09-13

Environment: Hyprland 0.56.2, Wayland, eDP-1 at 2880×1800 and display scale 1.5.
Test binary: `target/debug/hyprburst`, built with `cargo build --all-targets`.

An isolated `XDG_CONFIG_HOME` selected a Swatches v1 theme with
`[font] family = "DejaVu Sans Mono"`. Config used `font.size = 20.0`,
`ui.show_icons = true`, and an opaque window. No explicit font path or
`HYPRBURST_FONT` override was set. Isolated XDG data directories contained
three harmless desktop entries: Files (`files`), Firefox (`firefox`), and
Terminal (`terminal`), each with `Exec=true`. No entry was launched.
`HYPRBURST_CHILD=1` bypassed compositor redispatch without altering user config.

| Case | Native (`hyprburst native`) | Rio (`hyprburst`) |
| --- | --- | --- |
| Shared ordinary text family, installed Nerd Fonts | Pass: text retained; folder, Firefox, terminal icons visible and aligned | Pass: same text and icon rendering |
| Same theme, fontconfig restricted to DejaVu Sans Mono only | Pass: all three icons become four-tile app badges, no tofu | Pass: same generic badges, no tofu |

For the second case, `FONTCONFIG_FILE` pointed to an isolated configuration
whose only font directory contained DejaVu Sans Mono; `fc-list` confirmed that
no Nerd Font was exposed. No hardcoded Nerd Font path existed on this host.

Each window was allowed to render for one second after mapping, captured with
`grim`, and visually inspected. All four runs had empty stdout/stderr logs.
Test processes were terminated after capture; this smoke did not test app
launching or graceful close. An initial capture script's legacy Hyprland close
command was rejected by the Lua dispatcher; subsequent runs used targeted
process termination instead. No compositor settings were changed.

Local captures and the reproduction script are under
`/tmp/hyprburst-font-smoke.wct1Ka/` (`native.png`, `rio.png`,
`native-no-icons.png`, `rio-no-icons.png`, `smoke.py`). These are temporary local
artifacts, not packaged assets. Normal and fractional/scaled raster sizes are
also covered by the deterministic tests above; the live display stayed at 1.5×.
