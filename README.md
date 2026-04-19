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
│   ├── launcher.rs      # Launcher state, filtering, Hyprland dispatch
│   ├── search.rs        # Hybrid fuzzy/prefix ranking
│   └── terminal.rs      # Terminal lifecycle (raw mode, alternate screen, panic restore)
├── packaging/
│   ├── burst-launch     # Shell wrapper that execs burst inside the user's terminal
│   └── burst.desktop    # Desktop entry (StartupWMClass=burst)
├── .github/workflows/
│   └── ci.yml           # Pull request CI: fmt, clippy, tests
├── Cargo.toml
└── Cargo.lock
```

## Requirements

- **Nerd Font** — burst renders entry icons as Nerd Font glyphs in the private-use Unicode area. The hosting terminal must use a [Nerd Font](https://www.nerdfonts.com/) (e.g. `JetBrainsMono Nerd Font`, `FiraCode Nerd Font`, `Symbols Nerd Font`) or the icons will show as tofu squares.

## Hyprland Setup

Burst is a terminal UI, so the Hyprland window class belongs to the terminal that hosts it. Launch your terminal with a dedicated class (e.g. `burst`) so these rules target only burst windows:

```ini
# ~/.config/hypr/hyprland.conf
# Requires Hyprland 0.48+ (unified `windowrule` — `windowrulev2` is deprecated).

# Fullscreen overlay + semi-transparent blur for burst.
windowrule = match:class ^(burst)$, float on
windowrule = match:class ^(burst)$, size 100% 100%
windowrule = match:class ^(burst)$, center on
windowrule = match:class ^(burst)$, opacity 0.9 0.8
windowrule = match:class ^(burst)$, border_size 0
windowrule = match:class ^(burst)$, no_shadow on
windowrule = match:class ^(burst)$, stay_focused on
windowrule = match:class ^(burst)$, dim_around on

# Bind Super+Space to burst-launch, which picks your terminal automatically
# and passes the right --class / --app-id flag so the rules above match.
bind = SUPER, Space, exec, burst-launch

# Or call your terminal directly if you prefer to pin one:
# bind = SUPER, Space, exec, alacritty --class=burst -e burst
# bind = SUPER, Space, exec, wezterm   start --class=burst -- burst
# bind = SUPER, Space, exec, ghostty   --class=burst -e burst
# bind = SUPER, Space, exec, kitty     --class=burst burst
# bind = SUPER, Space, exec, foot      --app-id=burst burst
```

### Picking the host terminal

`burst-launch` resolves the terminal in this order:

1. `$TERMINAL` if it's set and on `PATH`.
2. Fallback chain: `alacritty → wezterm → ghostty → kitty → foot` (first found wins).

To pin a specific terminal without editing the script, export `TERMINAL` from your shell rc — for example:

```sh
# ~/.config/fish/config.fish
set -x TERMINAL wezterm

# or ~/.bashrc / ~/.zshrc
export TERMINAL=wezterm
```

The script knows the correct class/app-id flag for `alacritty`, `wezterm`, `ghostty`, `kitty`, and `foot`; anything else is invoked as `$TERMINAL -e burst` and may need you to pass the flag yourself in your Hyprland bind.

`burst-launch` lives at [`packaging/burst-launch`](packaging/burst-launch). Copy it onto your `$PATH` (e.g. `install -Dm755 packaging/burst-launch ~/.local/bin/burst-launch`) and optionally install [`packaging/burst.desktop`](packaging/burst.desktop) to `~/.local/share/applications/` so burst shows up in desktop-entry consumers.

The `opacity 0.9 0.8` rule sets active/inactive opacity; Hyprland's background blur applies automatically to the transparent regions as long as `decoration:blur:enabled = true` is set globally — terminals render on a transparent-capable background so the blur shows through. `stay_focused on` keeps the overlay focused, and `dim_around on` dims the rest of the screen while burst is open.

Burst renders a fade-in animation on open (configurable timing lives in `src/effects.rs`). Escape closes the overlay.

## Config

Default location: `$XDG_CONFIG_HOME/burst/config.toml`, falling back to `~/.config/burst/config.toml`. No config file is required — burst ships with sensible defaults and an invalid config is reported on stderr before falling back to them.

A fully-commented template lives at [`config.example.toml`](config.example.toml). Copy it and uncomment the fields you want to change:

```bash
mkdir -p ~/.config/burst
cp config.example.toml ~/.config/burst/config.toml
```

### Fields

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `ui.banner` | string | built-in ASCII "burst" | Multi-line TOML string (`"""..."""`). Empty string hides the banner. |
| `ui.prompt` | string | `"> "` | Printed before the search cursor. |
| `ui.page_size` | integer | `10` | Entries per page (PageUp/PageDown step). Must be `>= 1`. |
| `ui.show_icons` | bool | `true` | Draw a Nerd Font glyph before each app. Disable on non-Nerd-Font terminals. |
| `ui.selected_marker` | string | `"> "` | Prefix drawn on the selected row. |
| `ui.cursor_char` | string | `"█"` | Single-character cursor glyph after the prompt. |
| `ui.show_cursor` | bool | `true` | Draw the cursor glyph at all. |
| `colors.banner` | color | `magenta` | ASCII banner color. |
| `colors.prompt` | color | `cyan` | Prompt + cursor color. |
| `colors.selected` | color | `yellow` | Highlighted entry in the result list. |
| `colors.empty` | color | `yellow` | "No matches" message color. |

### Color values

Each color accepts either a **named color** (case-insensitive, dashes/underscores ignored) or a **6-digit hex string** like `#ff79c6`.

Named colors: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `gray`/`grey`, `darkgray`/`darkgrey`, `light-red`, `light-green`, `light-yellow`, `light-blue`, `light-magenta`, `light-cyan`, `reset` (terminal default).

### Minimal example

```toml
[ui]
prompt = "λ "
page_size = 8

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

Burst is tuned for instant launch. The release profile enables `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`, and `panic = "abort"` (see [`Cargo.toml`](Cargo.toml)) to minimize binary size and cold-start latency.

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
