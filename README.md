# Burst

A fast, fullscreen application launcher for Arch Linux + Hyprland, written in Rust.

Burst lives inside your terminal emulator with a semi-transparent blurred background, displays apps in a grid with icons, uses hybrid fuzzy/prefix search with recency+frequency scoring, and tracks launch history in SQLite for smart ranking.

## Features

- **Modern terminal aesthetic** — Clean monospace, subtle accent colors, custom ASCII art banners
- **Fast** — <50ms startup, instant search results as you type
- **Smart ranking** — Results ranked by recency (exponential decay) + frequency (launch count)
- **Icon grid** — Apps displayed with monochrome [Nerd Font](https://www.nerdfonts.com/) glyphs matched by category (browser, terminal, editor, etc.)
- **SQLite history** — Tracks launch history for smarter results and usage stats
- **TOML config** — Customizable colors, banner, and settings at `~/.config/burst/config.toml`
- **Hyprland native** — Launches apps via `hyprctl dispatch exec`

## Project Structure

```
burst/
├── src/
│   ├── main.rs          # Entry point, TUI event loop
│   ├── app.rs           # Application state and widget rendering
│   ├── config.rs        # TOML config + XDG path resolution
│   ├── desktop.rs       # .desktop file discovery and parsing
│   ├── effects.rs       # Fade-in animation via tachyonfx
│   ├── history.rs       # SQLite-backed launch history
│   ├── icon.rs          # Nerd Font glyph mapping by keyword (browser, terminal, editor, ...)
│   ├── input.rs         # Keyboard input handling and event polling
│   ├── launcher.rs           # Launcher state, filtering, Hyprland dispatch
│   ├── layout.rs             # Pure layout geometry (list + grid modes)
│   ├── search.rs             # Hybrid fuzzy/prefix ranking
│   ├── terminal.rs           # Terminal lifecycle (raw mode, alternate screen, panic restore)
│   └── terminal_resolver.rs  # Deterministic host-terminal resolution for bare `burst`
├── packaging/
│   └── hyprland-burst.conf   # Drop-in Hyprland config: windowrules + Super+Space bind
├── .github/workflows/
│   └── ci.yml           # Pull request CI: fmt, clippy, tests
├── Cargo.toml
└── Cargo.lock
```

## Requirements

- **Nerd Font** — burst renders entry icons as Nerd Font glyphs in the private-use Unicode area. The hosting terminal must use a [Nerd Font](https://www.nerdfonts.com/) (e.g. `JetBrainsMono Nerd Font`, `FiraCode Nerd Font`, `Symbols Nerd Font`) or the icons will show as tofu squares.

## Hyprland Setup

Burst is a terminal UI, so the Hyprland window class belongs to the terminal that hosts it. A ready-to-use config lives at [`packaging/hyprland-burst.conf`](packaging/hyprland-burst.conf) — it contains the windowrules that target the `burst` class and a `Super+Space` bind that runs `burst`, which resolves a terminal and re-execs itself inside it with the right class flag. (Use `burst tui` if you want to run inline in the current terminal instead.)

```sh
# Drop the config next to hyprland.conf and source it.
install -Dm644 packaging/hyprland-burst.conf ~/.config/hypr/hyprland-burst.conf
echo 'source = ~/.config/hypr/hyprland-burst.conf' >> ~/.config/hypr/hyprland.conf
```

The rules inside:

```ini
# Requires Hyprland 0.48+ (unified `windowrule`; `windowrulev2` is deprecated).
windowrule = match:class ^(burst)$, float on
windowrule = match:class ^(burst)$, size (monitor_w) (monitor_h)
windowrule = match:class ^(burst)$, move 0 0
windowrule = match:class ^(burst)$, opacity 0.9 0.8
windowrule = match:class ^(burst)$, border_size 0
windowrule = match:class ^(burst)$, no_shadow on
windowrule = match:class ^(burst)$, stay_focused on
windowrule = match:class ^(burst)$, dim_around on

bind = SUPER, Space, exec, burst

# env = TERMINAL,rio
```

The `size (monitor_w) (monitor_h)` and `move 0 0` rules make burst a full-monitor floating overlay without putting the client into real fullscreen. This matters for transparent terminals: the windows behind burst stay visible for opacity and blur effects. The `opacity 0.9 0.8` rule sets active/inactive opacity; Hyprland's background blur applies automatically to the transparent regions as long as `decoration:blur:enabled = true` is set globally — terminals render on a transparent-capable background so the blur shows through. `stay_focused on` keeps the overlay focused, and `dim_around on` dims the rest of the screen while burst is open.

If Waybar renders above burst, set Waybar's own config to `"layer": "bottom"` and restart Waybar:

```jsonc
{
  "layer": "bottom"
}
```

Waybar is a layer-shell surface, so a normal floating window rule cannot draw above it while Waybar remains on the `top` layer.

Burst renders a fade-in animation on open (configurable timing lives in `src/effects.rs`). Escape closes the overlay.

### Upgrading from earlier versions

> **Hard break — the `launch` subcommand is gone.** Running bare `burst` now re-execs into the resolved terminal (previously: `burst launch`), and `burst tui` runs inline in the current terminal (previously: bare `burst`). Update your Hyprland bind to `bind = SUPER, Space, exec, burst` or the launcher will fail when invoked from a graphical context with no controlling terminal.

## Terminal resolution

Bare `burst` picks the terminal to host the TUI deterministically. First match wins:

1. `terminal.preferred` in your config — each name, in order, is probed on `PATH`.
2. `$TERMINAL` if it's set and on `PATH`.
3. `$TERM`, then `$TERM_PROGRAM` (skipped if empty).
4. `x-terminal-emulator` (symlink-resolved to its real target, e.g. Debian's alternatives system).
5. The built-in fallback chain: `alacritty → wezterm → ghostty → kitty → foot → rio` (first found wins).

If none of those resolve, `burst` exits with a non-zero status and an error on stderr naming the built-in chain.

The `[terminal]` config section controls the first step and the invocation flags for each known emulator:

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `terminal.preferred` | array of strings | `[]` | Ordered preference list. First entry found on `PATH` wins over `$TERMINAL` and every fallback. |
| `terminal.class` | string | `"burst"` | Substituted into the `{class}` placeholder inside every flag template. Must be non-empty. |
| `terminal.flags.<name>.args` | array of strings | built-in table | Argv template for emulator `<name>`. Two placeholders are recognised: `{class}` (→ `terminal.class`) and `{cmd}` (→ `burst`). `{cmd}` is required — templates missing it are rejected with a warning and fall back to the built-in. |

The built-in flag table:

```toml
[terminal.flags.alacritty]
args = ["--class={class}", "-e", "{cmd}"]

[terminal.flags.wezterm]
args = ["start", "--class={class}", "--", "{cmd}"]

[terminal.flags.ghostty]
args = ["--class={class}", "-e", "{cmd}"]

[terminal.flags.kitty]
args = ["--class={class}", "{cmd}"]

[terminal.flags.foot]
args = ["--app-id={class}", "{cmd}"]

[terminal.flags.rio]
args = ["--title={class}", "-e", "{cmd}"]
```

Any emulator not listed here falls back to `-e {cmd}`, which works for most xterm-style terminals but means the window class may not match the rules above — add an entry under `[terminal.flags.<your-term>]` to fix that.

### Example: pin rio with a custom class

```toml
[terminal]
preferred = ["rio", "ghostty"]
class = "my-launcher"

[terminal.flags.rio]
args = ["--title={class}", "-e", "{cmd}"]
```

Remember to update the windowrules in `hyprland-burst.conf` to match the new class (e.g. `match:class ^(my-launcher)$`).

## Customizing the look

Two sections cover everything about how burst renders on screen: `[layout]` controls geometry and `[ui]` controls per-row decoration.

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
| `ui.banner` | string | built-in ASCII "burst" | Multi-line TOML string (`"""..."""`). Empty string hides the banner. |
| `ui.prompt` | string | `"> "` | Printed before the search cursor. |
| `ui.page_size` | integer | `10` | Entries per page (PageUp/PageDown step). Must be `>= 1`. |
| `ui.show_icons` | bool | `true` | Draw a Nerd Font glyph before each app. Disable on non-Nerd-Font terminals. |
| `ui.selected_marker` | string | `"> "` | Prefix drawn on the selected row. Empty string falls back to the default. |
| `ui.cursor_char` | string | `"█"` | Single-character cursor glyph after the prompt. Non-single-grapheme values fall back to the default. |
| `ui.show_cursor` | bool | `true` | Draw the cursor glyph at all. |
| `ui.loading_polish` | bool | `true` | Show a loading message while discovery is still running and smooth the handoff into results. Set to `false` for the most minimal loading screen. |

### Loading behavior

Burst starts application discovery immediately. If discovery finishes within the fast-start grace window, the launcher opens directly to the results and skips the loading screen entirely. If discovery takes longer, burst keeps the prompt responsive, shows a short loading message, and carries any typed query into the results when discovery completes.

With `ui.loading_polish = true`, the transition from loading to results is softened visually. Result updates after typing also transition only within the result area so the prompt and surrounding layout stay readable. Set `ui.loading_polish = false` under `[ui]` to hide the loading message and skip the loading-to-results polish while keeping startup behavior unchanged. If it is placed under `[layout]`, the config is invalid and burst falls back to built-in defaults.

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

## Environment variables (via Hyprland)

Burst reads **exactly one environment variable** in v1: `$TERMINAL`. Every other knob lives in `~/.config/burst/config.toml`. `$TERM` and `$TERM_PROGRAM` are consulted only as fallbacks during terminal resolution (see above) and have no effect on the TUI itself.

If you want to pin burst's host terminal without touching your shell rc, set `env` in Hyprland so the variable is exported to every process Hyprland spawns:

```ini
# ~/.config/hypr/hyprland.conf — or inside hyprland-burst.conf
env = TERMINAL,rio
```

For a project-scoped alternative, `terminal.preferred` in the config file wins over `$TERMINAL` anyway, so you can skip the env var entirely when you have a config file.

## Config

Default location: `$XDG_CONFIG_HOME/burst/config.toml`, falling back to `~/.config/burst/config.toml`. No config file is required — burst ships with sensible defaults and an invalid config is reported on stderr before falling back to them.

A fully-commented template lives at [`config.example.toml`](config.example.toml). Copy it and uncomment the fields you want to change:

```bash
mkdir -p ~/.config/burst
cp config.example.toml ~/.config/burst/config.toml
```

### Fields

The `[ui]`, `[layout]`, and `[terminal]` sections are documented in [Customizing the look](#customizing-the-look) and [Terminal resolution](#terminal-resolution). The remaining section controls colors:

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `colors.banner` | color | `magenta` | ASCII banner color. |
| `colors.prompt` | color | `cyan` | Prompt + cursor color. |
| `colors.selected` | color | `yellow` | Highlighted entry in the result list. |
| `colors.empty` | color | `yellow` | "No matches" message color. |

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
loading_polish = false

[colors]
banner   = "#ff79c6"
selected = "light-cyan"
```

### Validation

Unknown top-level keys, unknown keys in any section, malformed hex (`#fff`, `#xyzxyz`), unknown color names, and `ui.page_size = 0` are all rejected with a message naming the offending field. On any error burst prints the reason to stderr and starts with the built-in defaults.

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

Burst is tuned for instant launch. The release profile enables `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`, and `panic = "abort"` (see [`Cargo.toml`](Cargo.toml)) to minimize binary size and cold-start latency. Loading polish is cosmetic only; keep using the startup benchmark to catch discovery, config, or history regressions that a smoother handoff could otherwise make less obvious.

Measure startup end-to-end on your machine:

```bash
cargo build --release
./target/release/burst --bench-startup
```

This times the complete cold path — config load, `.desktop` discovery, history open — and prints peak resident-set size. On a reference Arch + Hyprland machine the output is:

```
burst startup: ~4ms
burst peak RSS: ~5 MB
```

Both well under the <50ms startup budget and minimal-memory goal.

CI asserts the same path stays under a **250ms ceiling** via `tests/bench_startup.rs` — generous headroom over the ~50ms local goal so shared CI runners don't flake. If the test ever does flake, it will be moved behind `#[ignore]` and run via `cargo test -- --ignored` as an opt-in lane.

## License

MIT
