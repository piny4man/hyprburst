# Changelog

All notable changes to Hyprburst are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `[window] placement` (`fullscreen`, `centered`) for Hyprland 0.55+ Lua
  launch-time rules driven from `~/.config/hyprburst/config.toml`; `[window]
  opacity` now also drives the generated Hyprland opacity rule.

### Changed

- `packaging/hyprburst.lua` is now a minimal `Super+Space` bind. Lua users no
  longer maintain static Hyprland window rules for placement; hyprburst relaunches
  itself once through `hl.dsp.exec_cmd(..., rules)` using TOML settings.

## [0.5.0] - 2026-06-01

Hyprburst now opens its **own GPU-rendered window** instead of spawning a
terminal emulator. The launcher owns its Wayland surface (winit + OpenGL),
painting the same ratatui layout directly, so the blur/transparency works and
there is no terminal to guess or re-exec.

### Added

- Native GPU launcher window (`src/window.rs`): a winit window with a
  hand-written OpenGL cell renderer (glutin + glow + `ab_glyph`) that paints
  `render_core`'s ratatui buffer directly. Bare `hyprburst` opens it.
- `[window]` config section: `app_id`, `width`, `height`, `transparent`.
- `[font]` config section: `path` (explicit `.ttf`/`.otf`) and `size`; the
  window otherwise resolves the system monospace via `fc-match`, with
  `$HYPRBURST_FONT` as an override.
- `colors.background` and `colors.foreground` for the window's clear color and
  default text color.
- `packaging/hyprburst.lua` — Hyprland 0.55+ Lua drop-in (`hl.window_rule` +
  `hl.bind`), alongside the existing hyprlang `packaging/hyprburst.conf`. The two
  formats are mutually exclusive; ship both so users pick the one matching their
  Hyprland config.
- `hyprburst --measure` opens the window and reports cold-start + peak RSS at the
  first presented frame.

### Changed

- The Hyprland windowrules now match hyprburst's own app-id (`window.app_id`,
  default `hyprburst`) instead of a hosting terminal's class.
- `hyprburst tui` is now explicitly the crossterm fallback for SSH / no-GPU
  sessions; bare `hyprburst` no longer runs inline.

### Removed

- **Breaking:** the `[terminal]` config section (`preferred`, `class`, `flags`)
  and all terminal-resolution logic. A config that still contains `[terminal]`
  is rejected with a migration hint — move `terminal.class` to `window.app_id`
  and delete the rest.
- The undocumented-but-unimplemented `ui.loading_polish` key (it never had an
  effect) is gone from the example config.

## [0.4.3] - 2026-05-31

Initial public release preparation. Hyprburst is a fast, fullscreen terminal
application launcher for Arch Linux and Hyprland.

### Added

- Hybrid fuzzy/prefix application search with recency + frequency ranking backed
  by a SQLite launch history.
- List and grid layout modes with column-aware navigation and configurable
  padding, banner centring, and separator line.
- Strictly validated TOML config (`[colors]`, `[terminal]`, `[layout]`, `[ui]`)
  at `~/.config/hyprburst/config.toml`; unknown keys are rejected loudly.
- Deterministic host-terminal resolution for bare `hyprburst`, re-execing into
  the user's preferred emulator (`alacritty`, `wezterm`, `ghostty`, `kitty`,
  `foot`, `rio`).
- `hyprburst tui` to run the launcher inline, and `hyprburst --bench-startup` to
  time cold startup and report peak RSS.
- Drop-in Hyprland overlay config at `packaging/hyprburst.conf` (full-monitor
  floating windowrules + a `Super+Space` bind).
- Arch/AUR packaging under `packaging/aur/` (`PKGBUILD` + `.SRCINFO`) that builds
  the published crate from source and installs the binary, the Hyprland config,
  and the example config.
- Loading polish and result-area query transitions powered by `tachyonfx`.
- Public-readiness docs: README, RELEASING, MAINTAINERS, CONTRIBUTING, SECURITY,
  CODE_OF_CONDUCT, and this changelog.

### Changed

- Published under the crate name `hyprburst` (the bare `burst` name was already
  taken on crates.io by an unrelated disassembler). The installed binary, the
  Hyprland window class, and the config/data directories
  (`~/.config/hyprburst/`, `~/.local/share/hyprburst/`) all use `hyprburst`.

[Unreleased]: https://github.com/piny4man/hyprburst/compare/v0.4.3...HEAD
[0.4.3]: https://github.com/piny4man/hyprburst/releases/tag/v0.4.3
