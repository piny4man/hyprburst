# Burst

A fast, fullscreen application launcher for Arch Linux + Hyprland, written in Rust.

Burst lives inside your terminal emulator with a semi-transparent blurred background, displays apps in a grid with icons, uses hybrid fuzzy/prefix search with recency+frequency scoring, and tracks launch history in SQLite for smart ranking.

## Features

- **Modern terminal aesthetic** — Clean monospace, subtle accent colors, custom ASCII art banners
- **Fast** — <50ms startup, instant search results as you type
- **Smart ranking** — Results ranked by recency (exponential decay) + frequency (launch count)
- **Icon grid** — Apps displayed with icons via kitty graphics protocol, sixel, or unicode/emoji fallback
- **SQLite history** — Tracks launch history for smarter results and usage stats
- **TOML config** — Customizable colors, banner, and settings at `~/.config/burst/config.toml`
- **Hyprland native** — Launches apps via `hyprctl dispatch exec`

## Project Structure

```
burst/
├── src/
│   ├── main.rs          # Entry point, TUI event loop
│   ├── app.rs           # Application state and widget rendering
│   ├── input.rs         # Keyboard input handling and event polling
│   └── terminal.rs      # Terminal lifecycle (raw mode, alternate screen, panic restore)
├── .github/workflows/
│   └── ci.yml           # Pull request CI: fmt, clippy, tests
├── Cargo.toml
└── Cargo.lock
```

### Planned Modules

| Module | Purpose |
|--------|---------|
| `config` | TOML config loading via `serde` + `toml`, XDG-compliant path resolution |
| `desktop` | Parse `.desktop` files from standard paths, build searchable index |
| `search` | Hybrid fuzzy/prefix matching with recency+frequency scoring |
| `history` | SQLite-backed usage tracking via `rusqlite` |
| `ui` | ratatui-based TUI: banner, search bar, icon grid, animations |
| `icons` | Freedesktop icon resolution + kitty graphics/sixel/unicode fallback |
| `launcher` | Hyprland integration via `hyprctl dispatch exec` |
| `terminal` | Terminal capability detection (kitty graphics, sixel support) |

## Hyprland Setup

Add these window rules to your Hyprland config:

```ini
windowrulev2 = float, class:^(burst)$
windowrulev2 = size 100% 100%, class:^(burst)$
windowrulev2 = opacity 0.9 0.8, class:^(burst)$
windowrulev2 = blur, class:^(burst)$
```

Bind it to a key (e.g., `Super+Space`):

```ini
bind = SUPER, Space, exec, burst
```

## Config

Default config location: `~/.config/burst/config.toml`

Sensible defaults are built in — no config required to start using burst.

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
cargo run
```

Press `Escape` to close.

## License

MIT
