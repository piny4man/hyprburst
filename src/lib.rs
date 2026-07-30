//! hyprburst — a fast application launcher for Hyprland.
//!
//! The crate is grouped into a few clusters:
//!
//! - [`domain`] — the frontend-agnostic state machine, app discovery, search,
//!   history, icons, and config (no rendering).
//! - [`view`] — ratatui `Buffer` painting (`render_core` + layout) shared by both
//!   frontends.
//! - [`gpu`] — the Rio-backed default and direct native fallback.
//! - [`tui`] — the crossterm/ratatui fallback for SSH / no-GPU sessions.
//! - [`system`] — Hyprland integration.
//! - [`bench`] — the live `--measure` / `--bench-startup` footprint probes.

pub mod bench;
pub mod domain;
pub mod gpu;
pub mod system;
pub mod tui;
pub mod view;
