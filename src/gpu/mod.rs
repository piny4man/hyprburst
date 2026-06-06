//! The GPU window launcher.
//!
//! [`window`] owns a Wayland surface via winit + glutin and paints the shared
//! [`render_core`](crate::view::render::render_core) buffer through a hand-written
//! OpenGL cell renderer (glow draw + an `ab_glyph` glyph atlas). [`grid`] holds the
//! windowless renderer primitives (cell metrics, atlas keying); [`font`] resolves
//! the monospace/Nerd Font the window rasterizes glyphs from.

pub mod font;
pub mod grid;
pub mod window;
