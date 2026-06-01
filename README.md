# Hyprburst

A fast, fullscreen application launcher for Arch Linux + Hyprland, written in Rust.

Hyprburst opens its own GPU-rendered window with a semi-transparent blurred background — it owns its Wayland surface, so there's no terminal to spawn or guess. It displays apps in a grid with icons, uses hybrid fuzzy/prefix search with recency+frequency scoring, and tracks launch history in SQLite for smart ranking.

## Features

- **Native GPU window** — winit + OpenGL cell renderer paints the ratatui layout directly; owns its Wayland surface for proper blur/transparency, no terminal needed
- **Modern aesthetic** — Clean monospace, subtle accent colors, custom ASCII art banners
- **Fast** — <50ms startup, instant search results as you type
- **Smart ranking** — Results ranked by recency (exponential decay) + frequency (launch count)
- **Icon grid** — Apps displayed with monochrome [Nerd Font](https://www.nerdfonts.com/) glyphs matched by category (browser, terminal, editor, etc.)
- **SQLite history** — Tracks launch history for smarter results and usage stats
- **TOML config** — Customizable window, font, colors, banner, and layout at `~/.config/hyprburst/config.toml`
- **Hyprland native** — Launches apps via `hyprctl dispatch` (auto-detects the 0.55+ Lua `hl.dsp` form)
- **Terminal fallback** — `hyprburst tui` runs the launcher inline in any terminal for SSH / no-GPU sessions

## Demo

> _Screenshots and a short screen capture live here._ Hyprburst is a fullscreen
> overlay, so a still of the grid plus a few-second recording of type-to-filter
> and launch convey it best. Drop assets under `docs/` (e.g. `docs/demo.gif`,
> `docs/grid.png`) and link them in this section.

<!-- ![Hyprburst grid](docs/grid.png) -->
<!-- ![Type to filter](docs/demo.gif) -->

## Project Structure

```
hyprburst/
├── src/
│   ├── main.rs          # Entry point: window launch, `tui` fallback, bench modes
│   ├── window.rs        # GPU launcher window (winit + glutin + glow cell renderer)
│   ├── gui.rs           # Cell-grid bookkeeping: metrics, glyph atlas, dirty diff
│   ├── font.rs          # Monospace font resolution (config path / $HYPRBURST_FONT / fc-match)
│   ├── app.rs           # TUI application state and widget rendering (crossterm fallback)
│   ├── config.rs        # TOML config + XDG path resolution
│   ├── desktop.rs       # .desktop file discovery and parsing
│   ├── effects.rs       # Fade-in animation via tachyonfx
│   ├── history.rs       # SQLite-backed launch history
│   ├── hyprland.rs      # hyprctl dispatch (legacy + 0.55+ Lua `hl.dsp` forms)
│   ├── icon.rs          # Nerd Font glyph mapping by keyword (browser, terminal, editor, ...)
│   ├── input.rs         # Keyboard input handling and event polling (TUI fallback)
│   ├── launcher.rs      # Shared render_core + crossterm key mapping
│   ├── launcher_core.rs # Frontend-agnostic launcher state machine
│   ├── layout.rs        # Pure layout geometry (list + grid modes)
│   ├── search.rs        # Hybrid fuzzy/prefix ranking
│   └── terminal.rs      # Terminal lifecycle for the TUI fallback (raw mode, panic restore)
├── packaging/
│   ├── hyprburst.conf   # Hyprlang drop-in: windowrules + Super+Space bind
│   └── hyprburst.lua    # Hyprland 0.55+ Lua drop-in (same rules + bind)
├── .github/workflows/
│   └── ci.yml           # Pull request CI: fmt, clippy, tests
├── Cargo.toml
└── Cargo.lock
```

## Requirements

- **OS** — Linux with [Hyprland](https://hyprland.org/) (a Wayland session). Hyprburst opens its own GPU window and dispatches launches through `hyprctl`, so it expects a running Hyprland session.
- **OpenGL** — the launcher window is rendered with OpenGL via your GPU driver (Mesa or vendor). Virtually every Hyprland-capable machine already has this.
- **Hyprland 0.48+** — the shipped `hyprburst.conf` uses the unified `windowrule` syntax. On Hyprland 0.55+ (Lua config), use `hyprburst.lua` instead — see [Hyprland Setup](#hyprland-setup).
- **Nerd Font** — hyprburst renders entry icons as Nerd Font glyphs in the private-use Unicode area. The window picks the system monospace via `fc-match`; if that font isn't a [Nerd Font](https://www.nerdfonts.com/), set `[font] path` (or `$HYPRBURST_FONT`) to one (e.g. `JetBrainsMono Nerd Font`) or the icons show as tofu squares.

## Install

### From source

Build and install the `hyprburst` binary into `~/.cargo/bin` (make sure it's on your `PATH`):

```bash
git clone https://github.com/piny4man/hyprburst
cd hyprburst
cargo install --path .
```

To build without installing, use `cargo build --release` — the binary lands at `target/release/hyprburst`.

### From crates.io

```bash
cargo install hyprburst
```

### From the AUR

> _Available once the AUR package is published._ The package name is `hyprburst`. Once live, install it with your preferred AUR helper:

```bash
paru -S hyprburst   # or: yay -S hyprburst
```

The `PKGBUILD` lives at [`packaging/aur/`](packaging/aur/) — it builds the published crate from source and installs the binary, the drop-in Hyprland config, and the example config. See its [README](packaging/aur/README.md) to build or publish it yourself.

After installing, drop in the Hyprland config (see [Hyprland Setup](#hyprland-setup)) and bind a key to `hyprburst`.

## Usage

Hyprburst exposes a single binary with a tiny command surface. Run `hyprburst help` for the same summary.

| Command | What it does |
|---------|--------------|
| `hyprburst` | Opens the GPU-rendered launcher **window**. It owns its own Wayland surface, so it works from a graphical context with no controlling terminal — this is what the `Super+Space` Hyprland bind invokes. |
| `hyprburst tui` | Runs the launcher **inline** in the current terminal (crossterm), with no window. The fallback for SSH / no-GPU sessions, and handy for testing the UI from a shell. |
| `hyprburst help` (`-h`, `--help`) | Prints the usage summary and exits. |
| `hyprburst --measure` | Opens the window, prints cold-start latency + peak RSS at the first frame, then exits. |
| `hyprburst --bench-startup` | Times the cold startup path (config load + app init), prints peak RSS, and exits without opening the UI. See [Performance](#performance). |

Inside the launcher: type to filter, arrow keys (or PageUp/PageDown) to move, `Enter` to launch the selected app, `Escape` to close.

## Hyprland Setup

Hyprburst owns its Wayland surface with app-id `hyprburst`, so the windowrules target that app-id directly (no terminal in between). Ship config comes in **two formats — pick the one matching your Hyprland config**, because Hyprland loads `hyprland.lua` if present, otherwise `hyprland.conf`; the two cannot coexist:

- **hyprlang** (Hyprland 0.48–0.54, or any `hyprland.conf` setup): [`packaging/hyprburst.conf`](packaging/hyprburst.conf)
- **Lua** (Hyprland 0.55+ with `hyprland.lua`): [`packaging/hyprburst.lua`](packaging/hyprburst.lua)

Both contain the same windowrules (targeting the `hyprburst` app-id) and a `Super+Space` bind that opens the launcher window. Use `hyprburst tui` if you want to run inline in a terminal instead.

### hyprlang (`hyprburst.conf`)

```sh
# Drop the config next to hyprland.conf and source it.
install -Dm644 packaging/hyprburst.conf ~/.config/hypr/hyprburst.conf
echo 'source = ~/.config/hypr/hyprburst.conf' >> ~/.config/hypr/hyprland.conf
```

```ini
# Requires Hyprland 0.48+ (unified `windowrule`; `windowrulev2` is deprecated).
windowrule = match:class ^(hyprburst)$, float on
windowrule = match:class ^(hyprburst)$, size (monitor_w) (monitor_h)
windowrule = match:class ^(hyprburst)$, move 0 0
windowrule = match:class ^(hyprburst)$, opacity 0.9 0.8
windowrule = match:class ^(hyprburst)$, border_size 0
windowrule = match:class ^(hyprburst)$, no_shadow on
windowrule = match:class ^(hyprburst)$, stay_focused on
windowrule = match:class ^(hyprburst)$, dim_around on

bind = SUPER, Space, exec, hyprburst
```

### Lua (`hyprburst.lua`, Hyprland 0.55+)

```sh
install -Dm644 packaging/hyprburst.lua ~/.config/hypr/hyprburst.lua
# then add to ~/.config/hypr/hyprland.lua:
#   dofile(os.getenv("HOME") .. "/.config/hypr/hyprburst.lua")
```

```lua
hl.bind("SUPER + Space", hl.dsp.exec_cmd("hyprburst"))
hl.window_rule({ match = { class = "hyprburst" }, float = true })
hl.window_rule({ match = { class = "hyprburst" }, size = "monitor_w monitor_h" })
hl.window_rule({ match = { class = "hyprburst" }, move = "0 0" })
hl.window_rule({ match = { class = "hyprburst" }, opacity = "0.9 0.8" })
hl.window_rule({ match = { class = "hyprburst" }, border_size = 0 })
hl.window_rule({ match = { class = "hyprburst" }, no_shadow = true })
hl.window_rule({ match = { class = "hyprburst" }, stay_focused = true })
hl.window_rule({ match = { class = "hyprburst" }, dim_around = true })
```

The `size (monitor_w) (monitor_h)` and `move 0 0` rules make hyprburst a full-monitor floating overlay without putting the client into real fullscreen. This matters for the blur: the windows behind hyprburst stay visible for opacity and blur effects. The `opacity 0.9 0.8` rule sets active/inactive opacity; Hyprland's background blur applies automatically to hyprburst's transparent surface as long as `decoration:blur:enabled = true` is set globally (and `[window] transparent = true`, the default). `stay_focused on` keeps the overlay focused, and `dim_around on` dims the rest of the screen while hyprburst is open.

If Waybar renders above hyprburst, set Waybar's own config to `"layer": "bottom"` and restart Waybar:

```jsonc
{
  "layer": "bottom"
}
```

Waybar is a layer-shell surface, so a normal floating window rule cannot draw above it while Waybar remains on the `top` layer.

Hyprburst renders a fade-in animation on open (configurable timing lives in `src/effects.rs`). Escape closes the overlay.

### Upgrading from 0.4.x

> **Hard break — hyprburst no longer spawns a terminal.** Bare `hyprburst` now opens its **own GPU window** instead of re-execing into a resolved terminal emulator. The `[terminal]` config section (`preferred`, `class`, `flags`) and the terminal-resolution logic are **gone**; a config that still contains `[terminal]` is rejected with a migration hint on stderr. To migrate: move `terminal.class` to `window.app_id` (if you customized it) and delete the rest of `[terminal]`. The `Super+Space` bind (`bind = SUPER, Space, exec, hyprburst`) is unchanged. `hyprburst tui` still runs inline in the current terminal as a fallback.

## Window and font

The launcher window and its cell font are configured under `[window]` and `[font]`.

### `[window]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `window.app_id` | string | `"hyprburst"` | Wayland app-id; the Hyprland windowrules match it. Must be non-empty. Change it and update the `match:class`/`match = { class = ... }` rules to match. |
| `window.width` | integer | `640` | Initial window width in logical pixels. Must be `>= 1`. (The overlay windowrule resizes it to the monitor anyway.) |
| `window.height` | integer | `720` | Initial window height in logical pixels. Must be `>= 1`. |
| `window.transparent` | bool | `true` | Keep the surface transparent so Hyprland's blur shows through. Set `false` to paint an opaque `colors.background` instead. |

### `[font]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `font.path` | string | unset | Explicit `.ttf`/`.otf` path for the cell font. When unset, the system monospace (`fc-match monospace`) is used; `$HYPRBURST_FONT` overrides it. Set this to a Nerd Font to guarantee entry icons render. |
| `font.size` | float | `16.0` | Logical font height in pixels (before DPI scaling). The cell size is derived from the font's metrics at this size. |

## Customizing the look

Two sections cover everything about how hyprburst renders on screen: `[layout]` controls geometry and `[ui]` controls per-row decoration.

### `[layout]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `layout.mode` | string | `"list"` | `"list"` for one entry per row, `"grid"` for column-aware navigation. |
| `layout.min_column_width` | integer | `20` | Grid-mode only. Cells narrower than this collapse to fewer columns. Must be `>= 1`. |
| `layout.padding_horizontal` | integer | `4` | Extra columns of whitespace on the left and right. Capped at 32; oversized values warn and fall back. |
| `layout.padding_vertical` | integer | `2` | Extra rows of whitespace above and below. Capped at 32. |
| `layout.center_banner` | bool | `false` | Horizontally center the banner inside the available width. |
| `layout.separator` | bool | `false` | Draw a thin rule between banner/search and the result list. |

### `[ui]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `ui.banner` | string | built-in ASCII "hyprburst" | Multi-line TOML string (`"""..."""`). Empty string hides the banner. |
| `ui.prompt` | string | `"> "` | Printed before the search cursor. |
| `ui.page_size` | integer | `10` | Entries per page (PageUp/PageDown step). Must be `>= 1`. |
| `ui.show_icons` | bool | `true` | Draw a Nerd Font glyph before each app. Disable if your font lacks Nerd glyphs. |
| `ui.selected_marker` | string | `"> "` | Prefix drawn on the selected row. Empty string falls back to the default. |
| `ui.cursor_char` | string | `"█"` | Single-character cursor glyph after the prompt. Non-single-grapheme values fall back to the default. |
| `ui.show_cursor` | bool | `true` | Draw the cursor glyph at all. |

### Worked example: centered grid with breathing room

```toml
[layout]
mode = "grid"
min_column_width = 28
padding_horizontal = 6
padding_vertical = 2
center_banner = true
separator = true

[ui]
prompt = "λ "
selected_marker = "▶ "
cursor_char = "▏"
show_icons = true

[colors]
banner   = "#ff79c6"
prompt   = "light-cyan"
selected = "#ffb86c"
```

## Environment variables

Hyprburst reads one environment variable: `$HYPRBURST_FONT`, an optional path to a `.ttf`/`.otf` to use as the window's cell font (it overrides `fc-match`, and is itself overridden by `[font] path` in the config). Everything else lives in `~/.config/hyprburst/config.toml`. App launches are dispatched through `hyprctl`, which auto-detects the Hyprland dispatch form; set `HYPRBURST_DISPATCH=lua|legacy` to force it if detection ever misfires.

## Config

Default location: `$XDG_CONFIG_HOME/hyprburst/config.toml`, falling back to `~/.config/hyprburst/config.toml`. No config file is required — hyprburst ships with sensible defaults and an invalid config is reported on stderr before falling back to them.

A fully-commented template lives at [`config.example.toml`](config.example.toml). Copy it and uncomment the fields you want to change:

```bash
mkdir -p ~/.config/hyprburst
cp config.example.toml ~/.config/hyprburst/config.toml
```

### Fields

The `[window]` and `[font]` sections are documented in [Window and font](#window-and-font); `[ui]` and `[layout]` in [Customizing the look](#customizing-the-look). The remaining section controls colors:

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `colors.banner` | color | `magenta` | ASCII banner color. |
| `colors.prompt` | color | `cyan` | Prompt + cursor color. |
| `colors.selected` | color | `yellow` | Highlighted entry in the result list. |
| `colors.empty` | color | `yellow` | "No matches" message color. |
| `colors.background` | color | `#121218` | Window background — painted only when `window.transparent = false`; otherwise the surface stays transparent for the blur. |
| `colors.foreground` | color | `#dcdcdc` | Default text color for unstyled launcher text (the Reset color). |

### Color values

Each color accepts either a **named color** (case-insensitive, dashes/underscores ignored) or a **6-digit hex string** like `#ff79c6`.

Named colors: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `gray`/`grey`, `darkgray`/`darkgrey`, `light-red`, `light-green`, `light-yellow`, `light-blue`, `light-magenta`, `light-cyan`, `reset` (terminal default).

### Minimal example

```toml
[layout]
padding_horizontal = 0
padding_vertical = 0

[ui]
prompt = "λ "
page_size = 8

[colors]
banner   = "#ff79c6"
selected = "light-cyan"
```

### Validation

Unknown top-level keys, unknown keys in any section, malformed hex (`#fff`, `#xyzxyz`), unknown color names, and `ui.page_size = 0` are all rejected with a message naming the offending field. On any error hyprburst prints the reason to stderr and starts with the built-in defaults.

## History Schema

Launch history is stored in SQLite:

```sql
launches (
  id INTEGER PRIMARY KEY,
  desktop_id TEXT NOT NULL UNIQUE,
  app_name TEXT NOT NULL,
  launch_count INTEGER NOT NULL DEFAULT 1,
  last_used TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  first_used TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
)
```

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

Press `Escape` to close.

## Performance

Hyprburst is tuned for instant launch. The release profile enables `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`, and `panic = "abort"` (see [`Cargo.toml`](Cargo.toml)) to minimize binary size and cold-start latency. Use the startup benchmark to catch discovery, config, or history regressions, and `hyprburst --measure` to capture window time-to-first-frame.

Measure startup end-to-end on your machine:

```bash
cargo build --release
./target/release/hyprburst --bench-startup
```

This times the complete cold path — config load, `.desktop` discovery, history open — and prints peak resident-set size. On a reference Arch + Hyprland machine the output is:

```
hyprburst startup: ~4ms
hyprburst peak RSS: ~5 MB
```

Both well under the <50ms startup budget and minimal-memory goal.

CI asserts the same path stays under a **250ms ceiling** via `tests/bench_startup.rs` — generous headroom over the ~50ms local goal so shared CI runners don't flake. If the test ever does flake, it will be moved behind `#[ignore]` and run via `cargo test -- --ignored` as an opt-in lane.

## Troubleshooting

**Icons show as tofu squares (□).** The window's font lacks Nerd Font glyphs. Set `[font] path` (or `$HYPRBURST_FONT`) to a [Nerd Font](https://www.nerdfonts.com/) such as `JetBrainsMono Nerd Font`, or set `ui.show_icons = false` to drop icons entirely.

**The window doesn't open / `hyprburst` exits with a font or display error.** It needs a Wayland session with OpenGL. From an SSH or no-GPU session, run `hyprburst tui` instead (inline crossterm UI). If it can't find a font, set `[font] path`.

**The launcher opens in the wrong place / isn't fullscreen.** The Hyprland windowrules target the `hyprburst` app-id. If you changed `window.app_id` in your config, update the `match:class ^(hyprburst)$` (or Lua `match = { class = ... }`) rules to match. If you skipped the config entirely, install [`packaging/hyprburst.conf`](packaging/hyprburst.conf) (or `.lua`) as shown in [Hyprland Setup](#hyprland-setup).

**Waybar (or another bar) renders on top of hyprburst.** Waybar is a layer-shell surface on the `top` layer, which a normal floating window can't cover. Set Waybar's config to `"layer": "bottom"` and restart it — details in [Hyprland Setup](#hyprland-setup).

**Background blur / transparency doesn't show.** Keep `[window] transparent = true` (the default) and set Hyprland's `decoration:blur:enabled = true` globally. The `opacity 0.9 0.8` windowrule only affects hyprburst's own window.

**`Super+Space` does nothing.** Confirm the bind sources/loads correctly (`bind = SUPER, Space, exec, hyprburst`, or the Lua `hl.bind(...)`) and that `hyprburst` is on the `PATH` Hyprland sees.

**My config isn't taking effect.** An invalid config is rejected and hyprburst falls back to built-in defaults, printing the reason to stderr. Run `hyprburst tui` from a shell to see the message, then fix the named field. Note: the `[terminal]` section was removed in 0.5 and now errors — see [Upgrading from 0.4.x](#upgrading-from-04x). Unknown keys are hard errors. See [Validation](#validation).

## License

Hyprburst is licensed under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`). See [LICENSE](LICENSE).

Official releases are controlled by the project owner. See [MAINTAINERS.md](MAINTAINERS.md) for crates.io, AUR, and release ownership policy.
