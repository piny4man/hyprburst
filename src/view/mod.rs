//! Shared ratatui `Buffer` painting.
//!
//! [`render`](render::render_core) paints a [`LauncherCore`](crate::domain::launcher_core::LauncherCore)
//! into a ratatui [`Buffer`](ratatui::buffer::Buffer); [`layout`] computes the
//! banner/input/list rects. Both frontends consume this layer — the TUI renders
//! the buffer to the terminal, the GPU window translates it to glyph quads.

pub mod layout;
pub mod render;
