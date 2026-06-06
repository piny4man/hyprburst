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
│   ├── main.rs            # Entry point: window launch, `tui` fallback, bench modes
│   ├── lib.rs             # Module tree (domain / view / gpu / tui / system / bench)
│   ├── bench.rs           # Footprint probes for `--measure` / `--bench-startup`
│   ├── domain/            # Frontend-agnostic state + data (no rendering)
│   │   ├── launcher_core.rs # Launcher state machine
│   │   ├── config.rs       # TOML config + XDG path resolution
│   │   ├── search.rs       # Hybrid fuzzy/prefix ranking
│   │   ├── history.rs      # SQLite-backed launch history
│   │   ├── icon.rs         # Nerd Font glyph mapping by keyword (browser, editor, ...)
│   │   └── desktop.rs      # .desktop file discovery and parsing
│   ├── view/             # Shared ratatui-buffer painting (both frontends)
│   │   ├── render.rs       # render_core: paints the launcher into a Buffer
│   │   └── layout.rs       # Pure layout geometry (list + grid modes)
│   ├── gpu/              # GPU window frontend (the default `hyprburst`)
│   │   ├── window.rs       # winit + glutin + glow cell renderer; GL-native fade-in
│   │   ├── grid.rs         # Cell metrics, glyph atlas, grid geometry
│   │   └── font.rs         # Cell-font resolution (config / $HYPRBURST_FONT / Nerd Font)
│   ├── tui/              # Crossterm/ratatui fallback frontend (`hyprburst tui`)
│   │   ├── launcher.rs     # Launcher widget + crossterm key mapping
│   │   ├── app.rs          # TUI application state and event loop
│   │   ├── input.rs        # Keyboard input handling and event polling
│   │   ├── terminal.rs     # Terminal lifecycle (raw mode, panic restore)
│   │   └── effects.rs      # Fade-in animation via tachyonfx (TUI only)
│   └── system/
│       └── hyprland.rs     # hyprctl dispatch (legacy + 0.55+ Lua `hl.dsp` forms)
├── packaging/
│   ├── hyprburst.conf   # Legacy hyprlang drop-in: static windowrules + bind
│   └── hyprburst.lua    # Hyprland 0.55+ Lua bind; placement lives in TOML
├── .github/workflows/
│   └── ci.yml           # Pull request CI: fmt, clippy, tests
├── Cargo.toml
└── Cargo.lock
```

## Requirements

- **OS** — Linux with [Hyprland](https://hyprland.org/) (a Wayland session). Hyprburst opens its own GPU window and dispatches launches through `hyprctl`, so it expects a running Hyprland session.
- **OpenGL** — the launcher window is rendered with OpenGL via your GPU driver (Mesa or vendor). Virtually every Hyprland-capable machine already has this.
- **Hyprland 0.55+ recommended** — use the Lua bind snippet and configure placement/opacity in `~/.config/hyprburst/config.toml`. Legacy Hyprland 0.48–0.54 / `hyprland.conf` setups can use the shipped hyprlang `hyprburst.conf` with unified `windowrule` syntax.
- **Nerd Font** — hyprburst renders entry icons as Nerd Font glyphs in the private-use Unicode area. It auto-picks an installed [Nerd Font](https://www.nerdfonts.com/) (a *Mono* variant preferred); if none is installed, the icons show as tofu squares — install one (e.g. `JetBrainsMono Nerd Font`) or set `[font] path` / `$HYPRBURST_FONT` to one.

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

The `PKGBUILD` lives at [`packaging/aur/`](packaging/aur/) — it builds the published crate from source and installs the binary, both Hyprland snippets, and the example config. See its [README](packaging/aur/README.md) to build or publish it yourself.

After installing, add the matching Hyprland snippet (see [Hyprland Setup](#hyprland-setup)).

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

When opening an app from its `.desktop` file, Hyprburst removes freedesktop launch target placeholders such as `%f`, `%F`, `%u`, and `%U`. That matches normal app-launcher behavior for launches without a selected file or URL, so apps such as Inkscape start normally instead of receiving a literal placeholder argument.

## Hyprland Setup

Hyprburst opens its own Wayland window with app-id `hyprburst`; it does not run inside a terminal. Install one small Hyprland snippet, then customize behavior in `~/.config/hyprburst/config.toml`.

Pick the snippet matching your Hyprland config format. Hyprland loads `hyprland.lua` if present, otherwise `hyprland.conf`; the two formats cannot be mixed.

- **Lua** (Hyprland 0.55+ with `hyprland.lua`): [`packaging/hyprburst.lua`](packaging/hyprburst.lua)
- **hyprlang** (legacy Hyprland 0.48–0.54, or any `hyprland.conf` setup): [`packaging/hyprburst.conf`](packaging/hyprburst.conf)

On Hyprland 0.55+ Lua configs, the snippet only binds the key. Placement and opacity come from Hyprburst's TOML config.

### Lua (`hyprburst.lua`, Hyprland 0.55+)

```sh
install -Dm644 packaging/hyprburst.lua ~/.config/hypr/hyprburst.lua
# then add to ~/.config/hypr/hyprland.lua:
#   dofile(os.getenv("HOME") .. "/.config/hypr/hyprburst.lua")
```

```lua
hl.bind("SUPER + Space", hl.dsp.exec_cmd("hyprburst"))
```

### Configure Placement

For current Lua setups, use `~/.config/hyprburst/config.toml`:

```toml
[window]
placement = "fullscreen" # or "centered"
opacity = 0.85
```

`fullscreen` is the default full-monitor overlay. `centered` uses `[window] width` and `[window] height`.

Hyprburst translates those settings into launch-time Lua rules:

| Placement | Hyprland behavior |
|-----------|-------------------|
| `fullscreen` | `float`, `pin`, `stay_focused`, `size = { "monitor_w", "monitor_h" }`, `move = { 0, 0 }`, `opacity = "... override"` |
| `centered` | `float`, `pin`, `stay_focused`, `size = { window.width, window.height }`, `center = true` |

Hyprburst is still a normal Wayland window. `pin` and focus rules keep it above normal app windows, but layer-shell surfaces such as top-layer Waybar can still draw above it.

### hyprlang (`hyprburst.conf`, legacy)

```sh
# Drop the config next to hyprland.conf and source it.
install -Dm644 packaging/hyprburst.conf ~/.config/hypr/hyprburst.conf
echo 'source = ~/.config/hypr/hyprburst.conf' >> ~/.config/hypr/hyprland.conf
```

```ini
# Requires Hyprland 0.48-0.54 (unified `windowrule`; `windowrulev2` is deprecated).
windowrule = match:class ^(hyprburst)$, float on
windowrule = match:class ^(hyprburst)$, pin on
windowrule = match:class ^(hyprburst)$, size (monitor_w) (monitor_h)
windowrule = match:class ^(hyprburst)$, move 0 0
windowrule = match:class ^(hyprburst)$, opacity 0.9 0.9
windowrule = match:class ^(hyprburst)$, border_size 0
windowrule = match:class ^(hyprburst)$, no_shadow on
windowrule = match:class ^(hyprburst)$, stay_focused on
windowrule = match:class ^(hyprburst)$, dim_around on

bind = SUPER, Space, exec, hyprburst
```

The legacy hyprlang file keeps static rules because old hyprlang configs cannot use the Lua `hl.dsp.exec_cmd("hyprburst", rules)` launch-time rule path. On current Hyprland Lua configs, prefer the minimal Lua bind plus TOML placement above.

If Waybar renders above hyprburst, set Waybar's own config to `"layer": "bottom"` and restart Waybar:

```jsonc
{
  "layer": "bottom"
}
```

Waybar is a layer-shell surface, so a normal window cannot draw above it while Waybar remains on the `top` layer.

Hyprburst fades in on open. `Escape` closes the overlay.

### Upgrading from 0.4.x

> **Hard break — hyprburst no longer spawns a terminal.** Bare `hyprburst` now opens its **own GPU window** instead of re-execing into a resolved terminal emulator. The `[terminal]` config section (`preferred`, `class`, `flags`) and the terminal-resolution logic are **gone**; a config that still contains `[terminal]` is rejected with a migration hint on stderr. To migrate: move `terminal.class` to `window.app_id` (if you customized it) and delete the rest of `[terminal]`. The `Super+Space` bind (`bind = SUPER, Space, exec, hyprburst`) is unchanged. `hyprburst tui` still runs inline in the current terminal as a fallback.

## Window and font

The launcher window and cell font are configured under `[window]` and `[font]` in `~/.config/hyprburst/config.toml`.

### `[window]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `window.app_id` | string | `"hyprburst"` | Wayland app-id. Change only if you know why; legacy hyprlang rules must match it. |
| `window.placement` | string | `"fullscreen"` | `"fullscreen"` for full-monitor overlay, `"centered"` for a centered floating window. |
| `window.width` | integer | `640` | Centered window width. Must be `>= 1`. |
| `window.height` | integer | `720` | Centered window height. Must be `>= 1`. |
| `window.transparent` | bool | `true` | Keep transparent for Hyprland blur. Set `false` for an opaque `colors.background`. |
| `window.opacity` | float | `0.85` | Single opacity knob. On Lua Hyprland it is sent as compositor opacity with `override`; hyprburst also uses it for the transparent background panel. |

### `[font]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `font.path` | string | unset | Explicit `.ttf`/`.otf` path for the cell font. When unset, hyprburst auto-picks an installed **Nerd Font** (so icons render), falling back to the system monospace (`fc-match monospace`); `$HYPRBURST_FONT` overrides everything. If icons show as tofu, point this at a *Nerd Font Mono* file. |
| `font.size` | float | `20.0` | Logical font height in pixels (before DPI scaling). The cell size is derived from the font's metrics at this size. Bump it up if text looks too small. |

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

Hyprburst reads `$HYPRBURST_FONT`, an optional path to a `.ttf`/`.otf` to use as the window's cell font (it overrides `fc-match`, and is itself overridden by `[font] path` in the config). The window launcher uses `$HYPRBURST_CHILD` internally to avoid relaunch loops when applying Hyprland Lua launch rules. Everything else lives in `~/.config/hyprburst/config.toml`. App launches are dispatched through `hyprctl`, which auto-detects the Hyprland dispatch form; set `HYPRBURST_DISPATCH=lua|legacy` to force it if detection ever misfires.

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
| `colors.banner` | color | `#c6a0f6` | ASCII banner color. |
| `colors.prompt` | color | `#8aadf4` | Prompt + cursor color. |
| `colors.selected` | color | `#f2d5ff` | Text of the highlighted entry. |
| `colors.selected_bg` | color | `#3a2e5a` | Highlight bar drawn behind the selected row. |
| `colors.empty` | color | `#9a8cb5` | "No matches" message color. |
| `colors.background` | color | `#1a1b26` | Window background — painted opaque when `window.transparent = false`, and as the dimming panel (at `window.opacity`) when `transparent = true`. |
| `colors.foreground` | color | `#c8cce0` | Default text color for unstyled launcher text (the Reset color). |

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

**Icons show as tofu squares (□).** No Nerd Font is installed for hyprburst to auto-pick. Install a [Nerd Font](https://www.nerdfonts.com/) such as `JetBrainsMono Nerd Font` (the *Mono* variant keeps icons one cell wide), or point `[font] path` (or `$HYPRBURST_FONT`) at one, or set `ui.show_icons = false` to drop icons entirely.

**Text is too small.** Raise `[font] size` (default `20.0`).

**The window doesn't open / `hyprburst` exits with a font or display error.** It needs a Wayland session with OpenGL. From an SSH or no-GPU session, run `hyprburst tui` instead (inline crossterm UI). If it can't find a font, set `[font] path`.

**The launcher opens in the wrong place / isn't fullscreen.** On Hyprland 0.55+ Lua configs, install the minimal [`packaging/hyprburst.lua`](packaging/hyprburst.lua) bind and set `[window] placement` in `~/.config/hyprburst/config.toml`. Use `"fullscreen"` for the full-monitor overlay or `"centered"` for a centered floating window. Legacy hyprlang users still rely on static rules in [`packaging/hyprburst.conf`](packaging/hyprburst.conf).

**Waybar (or another bar) renders on top of hyprburst.** Waybar is a layer-shell surface on the `top` layer, which a normal floating window can't cover. Set Waybar's config to `"layer": "bottom"` and restart it — details in [Hyprland Setup](#hyprland-setup).

**Background blur / transparency doesn't show.** Keep `[window] transparent = true` (the default) and set Hyprland's `decoration:blur:enabled = true` globally. `[window] opacity` controls both Hyprland compositor opacity on Lua setups and the panel hyprburst paints inside its transparent surface.

**The launcher looks too see-through / the blur washes out the text.** Raise `[window] opacity` (default `0.85`) toward `1.0` to dim more of the blur behind the launcher; `1.0` makes the background panel fully opaque.

**`Super+Space` does nothing.** Confirm the bind sources/loads correctly (`bind = SUPER, Space, exec, hyprburst`, or the Lua `hl.bind(...)`) and that `hyprburst` is on the `PATH` Hyprland sees.

**My config isn't taking effect.** An invalid config is rejected and hyprburst falls back to built-in defaults, printing the reason to stderr. Run `hyprburst tui` from a shell to see the message, then fix the named field. Note: the `[terminal]` section was removed in 0.5 and now errors — see [Upgrading from 0.4.x](#upgrading-from-04x). Unknown keys are hard errors. See [Validation](#validation).

## License

Hyprburst is licensed under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`). See [LICENSE](LICENSE).

Official releases are controlled by the project owner. See [MAINTAINERS.md](MAINTAINERS.md) for crates.io, AUR, and release ownership policy.
