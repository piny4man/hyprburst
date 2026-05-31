# Changelog

All notable changes to Hyprburst are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
