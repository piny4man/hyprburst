//! Embedded-terminal POC support (Phase 5 of the Freya bake-off).
//!
//! The live binary `hyprburst-spike-term` opens a Freya window that hosts a PTY
//! running the **unmodified** `hyprburst tui` — owning the terminal host while
//! keeping the shipped ratatui codepath. This module holds the parts that don't
//! need a Freya runtime, so they can be unit-tested:
//!
//! - [`inner_binary_path`] / [`launcher_command`] — resolve and build the
//!   command the PTY spawns.
//! - [`ParseModel`] — a headless model of the variant's per-frame CPU cost for
//!   the benchmark harness: render the launcher exactly as the baseline does,
//!   serialize the frame to the VT bytes the PTY would carry, and advance an
//!   `alacritty_terminal` grid with them — the same parse Freya's terminal host
//!   runs on every chunk of output. The column therefore reflects inner render
//!   **plus** the emulator surcharge that sets this variant apart from the
//!   baseline; GPU compositing is excluded, as in every other column.

use std::path::{Path, PathBuf};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::Processor;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::launcher::render_core;
use crate::launcher_core::LauncherCore;

/// Inner binary the embedded PTY runs.
pub const INNER_BINARY: &str = "hyprburst";
/// Subcommand passed to the inner binary — run the TUI inline (no re-exec).
pub const INNER_ARGS: &[&str] = &["tui"];

/// Resolve the `hyprburst` binary the PTY should launch.
///
/// Prefers a sibling of the running executable, so `cargo run` and local
/// installs host the just-built binary rather than whatever is on `PATH`. Falls
/// back to the bare `hyprburst` name (resolved via `PATH` at spawn time) when no
/// sibling exists or the current exe is unknown.
pub fn inner_binary_path(current_exe: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = current_exe.as_deref().and_then(Path::parent) {
        let sibling = dir.join(INNER_BINARY);
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from(INNER_BINARY)
}

/// Build the PTY command that runs `hyprburst tui`, tagged as a 256-color
/// terminal so the inner TUI emits its full styling.
pub fn launcher_command() -> freya::terminal::CommandBuilder {
    let program = inner_binary_path(std::env::current_exe().ok());
    let mut cmd = freya::terminal::CommandBuilder::new(program);
    for arg in INNER_ARGS {
        cmd.arg(arg);
    }
    cmd.env("TERM", "xterm-256color");
    cmd
}

/// No-op listener: the headless harness ignores the emulator's side events
/// (bell, title, clipboard) — it only needs the parse to run.
struct SilentListener;
impl EventListener for SilentListener {}

/// [`Dimensions`] for the headless grid, sized to the harness render area.
struct GridDims {
    rows: usize,
    cols: usize,
}

impl Dimensions for GridDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Headless model of the embedded-terminal variant's per-frame CPU work: inner
/// ratatui render → VT serialization → emulator parse. See the module docs.
pub struct ParseModel {
    area: Rect,
    prev: Buffer,
    term: Term<SilentListener>,
    processor: Processor,
}

impl ParseModel {
    /// Build a model whose grid matches the render `area`.
    pub fn new(area: Rect) -> Self {
        let dims = GridDims {
            rows: area.height as usize,
            cols: area.width as usize,
        };
        let term = Term::new(TermConfig::default(), &dims, SilentListener);
        Self {
            area,
            prev: Buffer::empty(area),
            term,
            processor: Processor::new(),
        }
    }

    /// One frame: render the launcher, serialize the diff to PTY bytes, and feed
    /// them through the emulator — exactly the work the live host does per frame.
    pub fn paint(&mut self, core: &mut LauncherCore) {
        let mut cur = Buffer::empty(self.area);
        render_core(core, self.area, &mut cur);
        let bytes = serialize_diff(&self.prev, &cur);
        self.processor.advance(&mut self.term, &bytes);
        self.prev = cur;
    }

    /// Visible text currently in the emulator grid — for tests asserting that
    /// the serialized frame actually parsed into the terminal.
    #[cfg(test)]
    fn visible_text(&self) -> String {
        self.term.grid().display_iter().map(|cell| cell.c).collect()
    }
}

/// Serialize the cells that changed between `prev` and `cur` into the ANSI byte
/// stream ratatui writes over the PTY, using ratatui's own crossterm backend so
/// the bytes — and thus the parse cost — match the real frontend rather than an
/// invented encoding. Mirrors ratatui's incremental rendering: only the diff is
/// written, just as the live TUI would send over the wire.
pub fn serialize_diff(prev: &Buffer, cur: &Buffer) -> Vec<u8> {
    let updates = prev.diff(cur);
    let mut bytes = Vec::new();
    {
        // Writing into a `Vec` never fails; the backend is dropped at the end of
        // the scope, releasing the borrow so `bytes` can be returned.
        let mut backend = CrosstermBackend::new(&mut bytes);
        let _ = backend.draw(updates.into_iter());
        let _ = backend.flush();
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::synthetic_apps;
    use crate::config::Config;

    #[test]
    fn inner_binary_path_prefers_existing_sibling() {
        let dir = std::env::temp_dir().join(format!("hyprburst-term-host-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sibling = dir.join(INNER_BINARY);
        std::fs::write(&sibling, b"").unwrap();

        let exe = dir.join("hyprburst-spike-term");
        assert_eq!(inner_binary_path(Some(exe)), sibling);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inner_binary_path_falls_back_to_bare_name() {
        let exe = PathBuf::from("/no/such/dir/hyprburst-spike-term");
        assert_eq!(inner_binary_path(Some(exe)), PathBuf::from(INNER_BINARY));
        assert_eq!(inner_binary_path(None), PathBuf::from(INNER_BINARY));
    }

    #[test]
    fn serialize_diff_emits_rendered_launcher_text() {
        let area = Rect::new(0, 0, 80, 40);
        let mut core = LauncherCore::from_apps(synthetic_apps(), Config::default());
        let mut cur = Buffer::empty(area);
        render_core(&mut core, area, &mut cur);

        let bytes = serialize_diff(&Buffer::empty(area), &cur);
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("Firefox"),
            "serialized frame should carry the rendered app names",
        );
    }

    #[test]
    fn serialize_diff_resends_no_cell_content_when_nothing_changed() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let mut core = LauncherCore::from_apps(synthetic_apps(), Config::default());
        render_core(&mut core, area, &mut buf);
        // Same buffer on both sides: ratatui's incremental render re-sends only
        // the changed cells (none here), so no app text crosses the wire — even
        // though the backend still emits a tiny control prelude.
        let bytes = serialize_diff(&buf, &buf);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("Firefox"));
    }

    #[test]
    fn parse_model_paints_launcher_into_the_emulator_grid() {
        let area = Rect::new(0, 0, 80, 40);
        let mut core = LauncherCore::from_apps(synthetic_apps(), Config::default());
        let mut model = ParseModel::new(area);

        model.paint(&mut core);

        assert!(
            model.visible_text().contains("Firefox"),
            "the emulator grid should hold the parsed launcher frame",
        );
    }
}
