//! The crossterm/ratatui fallback launcher.
//!
//! Runs the launcher inline in the current terminal — the path for SSH / no-GPU
//! sessions where the [`gpu`](crate::gpu) window can't open. [`launcher`] maps
//! crossterm keys to launcher actions and wraps the shared
//! [`render_core`](crate::view::render::render_core); [`app`] composes it with the
//! fade-in [`effects`]; [`input`]/[`terminal`] handle crossterm I/O.

pub mod app;
pub mod effects;
pub mod input;
pub mod launcher;
pub mod terminal;
