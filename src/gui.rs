//! Cell-grid bookkeeping for the GL-rendered launcher window.
//!
//! The windowed launcher ([`crate::window`]) owns its rendering surface *without*
//! a terminal emulator: a winit window, a hand-written OpenGL cell renderer
//! (glutin context + glow draw + an `ab_glyph` glyph atlas), and an in-process
//! [`LauncherCore`]. Because we own the TUI, there is no PTY and no child process
//! — the launcher is painted with [`render_core`] straight into a ratatui
//! [`Buffer`] (which *is* a cell grid), and that buffer is drawn directly.
//!
//! A cell renderer is mostly bookkeeping that needs no GPU and no window, so that
//! bookkeeping lives here where it can be unit-tested:
//!
//! - [`CellMetrics`] / [`grid_size`] / [`cell_rect`] — map a window's pixel size
//!   and a monospace cell size to a grid and each cell's pixel rect.
//! - [`Atlas`] — keys rasterized glyphs into a fixed texture grid: each distinct
//!   [`GlyphKey`] is rasterized and uploaded at most once, then reused.
//! - [`dirty_cells`] — the dirty-region diff between two grid snapshots, so a
//!   frame redraws only what moved.
//!
//! The headless frame-cost model that fills the benchmark's `gui` column
//! ([`GuiModel`]) composes these with the same [`render_core`] the baseline and
//! live window use — see [`crate::bench::run_gui`]. GPU upload/draw and glyph
//! rasterization are excluded from the model (they happen in the live window),
//! exactly as GPU compositing is excluded from every other column.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use crate::launcher::render_core;
use crate::launcher_core::LauncherCore;

/// Pixel size of one monospace character cell. Every layout figure derives from
/// this and the window size; the live renderer reads it from the rasterized font
/// metrics, tests pass it explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetrics {
    pub cell_w: u32,
    pub cell_h: u32,
}

impl CellMetrics {
    /// A cell of the given pixel size, clamped to at least 1×1 so layout math
    /// never divides by zero on a degenerate font metric.
    pub fn new(cell_w: u32, cell_h: u32) -> Self {
        Self {
            cell_w: cell_w.max(1),
            cell_h: cell_h.max(1),
        }
    }
}

/// Grid dimensions in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

/// Map a window's pixel size to a grid: as many whole cells as fit, at least 1×1.
/// The remainder pixels are left as padding the renderer can letterbox.
pub fn grid_size(window_px: (u32, u32), cell: CellMetrics) -> GridSize {
    let cols = (window_px.0 / cell.cell_w).max(1);
    let rows = (window_px.1 / cell.cell_h).max(1);
    GridSize {
        // Grids beyond u16 are absurd for a launcher window; clamp rather than
        // wrap so a freak window size can't produce a tiny grid via truncation.
        cols: cols.min(u16::MAX as u32) as u16,
        rows: rows.min(u16::MAX as u32) as u16,
    }
}

/// Pixel rectangle `(x, y, w, h)` of the cell at `(col, row)`, top-left origin —
/// where the renderer blits that cell's glyph quad.
pub fn cell_rect(col: u16, row: u16, cell: CellMetrics) -> (u32, u32, u32, u32) {
    let x = col as u32 * cell.cell_w;
    let y = row as u32 * cell.cell_h;
    (x, y, cell.cell_w, cell.cell_h)
}

/// Identity of a rasterized glyph in the [`Atlas`]: the character plus the style
/// bits that change its pixels. Bold is a different rasterization, so it keys
/// separately; color does not (the shader tints a single white-on-clear glyph),
/// so it is deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub ch: char,
    pub bold: bool,
}

impl GlyphKey {
    pub fn new(ch: char, bold: bool) -> Self {
        Self { ch, bold }
    }
}

/// A slot returned by [`Atlas::get_or_insert`]: where the glyph lives in the
/// atlas texture and whether this call is the one that allocated it (so the live
/// renderer knows it must rasterize + upload now, versus reuse an existing tile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSlot {
    /// Linear slot index, row-major.
    pub index: u32,
    /// Top-left pixel of the slot's tile in the atlas texture.
    pub px: (u32, u32),
    /// `true` only on the call that first allocated this glyph.
    pub newly_inserted: bool,
}

/// A fixed-capacity glyph atlas: a texture of `atlas_px` divided into a grid of
/// `cell`-sized tiles. Each distinct [`GlyphKey`] is assigned one tile on first
/// sight and reused thereafter, so a glyph is rasterized and uploaded once. When
/// every tile is taken, further new glyphs return `None` (the spike's app set
/// fits comfortably; a production renderer would evict LRU — noted, not built).
pub struct Atlas {
    atlas_px: (u32, u32),
    cell: CellMetrics,
    cols: u32,
    capacity: u32,
    map: std::collections::HashMap<GlyphKey, u32>,
    next: u32,
}

impl Atlas {
    /// An empty atlas of `atlas_px` pixels tiled into `cell`-sized slots.
    pub fn new(atlas_px: (u32, u32), cell: CellMetrics) -> Self {
        let cols = (atlas_px.0 / cell.cell_w).max(1);
        let rows = (atlas_px.1 / cell.cell_h).max(1);
        Self {
            atlas_px,
            cell,
            cols,
            capacity: cols * rows,
            map: std::collections::HashMap::new(),
            next: 0,
        }
    }

    /// Look up `key`, allocating a tile on first sight. Returns the slot (with
    /// `newly_inserted` set when this call allocated it) or `None` if the atlas
    /// is full and `key` is not already resident.
    pub fn get_or_insert(&mut self, key: GlyphKey) -> Option<AtlasSlot> {
        if let Some(&index) = self.map.get(&key) {
            return Some(self.slot(index, false));
        }
        if self.next >= self.capacity {
            return None;
        }
        let index = self.next;
        self.next += 1;
        self.map.insert(key, index);
        Some(self.slot(index, true))
    }

    /// Build the [`AtlasSlot`] for a resident `index`.
    fn slot(&self, index: u32, newly_inserted: bool) -> AtlasSlot {
        let col = index % self.cols;
        let row = index / self.cols;
        AtlasSlot {
            index,
            px: (col * self.cell.cell_w, row * self.cell.cell_h),
            newly_inserted,
        }
    }

    /// Normalized `(u0, v0, u1, v1)` texture coordinates of a slot's tile — what
    /// the renderer hands the shader to sample the glyph quad.
    pub fn uv_rect(&self, slot: AtlasSlot) -> (f32, f32, f32, f32) {
        let (x, y) = slot.px;
        let u0 = x as f32 / self.atlas_px.0 as f32;
        let v0 = y as f32 / self.atlas_px.1 as f32;
        let u1 = (x + self.cell.cell_w) as f32 / self.atlas_px.0 as f32;
        let v1 = (y + self.cell.cell_h) as f32 / self.atlas_px.1 as f32;
        (u0, v0, u1, v1)
    }

    /// Distinct glyphs currently resident.
    pub fn len(&self) -> u32 {
        self.next
    }

    pub fn is_empty(&self) -> bool {
        self.next == 0
    }

    /// Total tiles the atlas can hold.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

/// One rendered cell's content. Equality drives the dirty diff: a cell is redrawn
/// iff any of these fields changed, so glyph, weight, and both colors participate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    pub ch: char,
    pub bold: bool,
    /// Packed foreground / background (see [`pack_color`]).
    pub fg: u32,
    pub bg: u32,
}

impl Default for GridCell {
    fn default() -> Self {
        // A blank cell: space, default weight, reset fg/bg — the cleared grid.
        Self {
            ch: ' ',
            bold: false,
            fg: pack_color(Color::Reset),
            bg: pack_color(Color::Reset),
        }
    }
}

impl GridCell {
    /// The [`GlyphKey`] this cell would sample from the atlas.
    pub fn glyph_key(&self) -> GlyphKey {
        GlyphKey::new(self.ch, self.bold)
    }
}

/// Indices of the cells that changed between two equally-sized grid snapshots —
/// the dirty region the renderer repaints. When the snapshots differ in length
/// (a resize), every cell of `cur` is dirty: the whole grid is repainted.
pub fn dirty_cells(prev: &[GridCell], cur: &[GridCell]) -> Vec<usize> {
    if prev.len() != cur.len() {
        return (0..cur.len()).collect();
    }
    cur.iter()
        .zip(prev)
        .enumerate()
        .filter_map(|(i, (c, p))| (c != p).then_some(i))
        .collect()
}

/// Pack a ratatui [`Color`] into a `u32` for the dirty diff. RGB maps to
/// `0xRRGGBB`; indexed and named variants are tagged above the 24-bit RGB range so
/// distinct color *kinds* never alias into a false cache hit (the live binary maps
/// colors to real RGB separately — the model only needs stable distinctness).
pub fn pack_color(color: Color) -> u32 {
    match color {
        Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        Color::Indexed(i) => 0x0200_0000 | i as u32,
        named => 0x0100_0000 | named_index(named),
    }
}

/// A small stable index per named ratatui color, for [`pack_color`]'s tagging.
fn named_index(color: Color) -> u32 {
    match color {
        Color::Reset => 0,
        Color::Black => 1,
        Color::Red => 2,
        Color::Green => 3,
        Color::Yellow => 4,
        Color::Blue => 5,
        Color::Magenta => 6,
        Color::Cyan => 7,
        Color::Gray => 8,
        Color::DarkGray => 9,
        Color::LightRed => 10,
        Color::LightGreen => 11,
        Color::LightYellow => 12,
        Color::LightBlue => 13,
        Color::LightMagenta => 14,
        Color::LightCyan => 15,
        Color::White => 16,
        // RGB/Indexed are handled before this is called; map defensively.
        _ => 17,
    }
}

/// Snapshot a ratatui [`Buffer`] into renderer cells, row-major over its area —
/// the grid the dirty diff and the live draw both consume.
pub fn snapshot_buffer(buf: &Buffer) -> Vec<GridCell> {
    buf.content()
        .iter()
        .map(|cell| GridCell {
            ch: cell.symbol().chars().next().unwrap_or(' '),
            bold: cell.modifier.contains(Modifier::BOLD),
            fg: pack_color(cell.fg),
            bg: pack_color(cell.bg),
        })
        .collect()
}

/// Cell size the headless model lays its grid out with. The exact pixels are
/// immaterial to the modelled CPU work (snapshot/diff/atlas-keying), so a
/// plausible monospace cell is fixed for determinism; the live renderer uses real
/// font metrics.
const MODEL_CELL: CellMetrics = CellMetrics {
    cell_w: 10,
    cell_h: 20,
};

/// Headless model of the GUI's per-frame CPU work: render the launcher with
/// [`render_core`] (identical to the baseline — the window paints the same TUI),
/// snapshot the resulting ratatui [`Buffer`] into renderer cells, diff it against
/// the previous frame ([`dirty_cells`]), and resolve each changed cell's glyph in
/// the [`Atlas`] (a cache hit after first sight). There is **no VT serialize and
/// no emulator parse** — owning the surface natively removes that surcharge.
/// Glyph rasterization and GL upload/draw are excluded — they happen in the live
/// window — as GPU compositing is from every other column.
pub struct GuiModel {
    area: Rect,
    atlas: Atlas,
    prev_snapshot: Vec<GridCell>,
}

impl GuiModel {
    /// Build a model whose grid matches the render `area`.
    pub fn new(area: Rect) -> Self {
        // A 1024² atlas tiled into MODEL_CELL slots holds thousands of glyphs —
        // far more than the launcher's character set ever needs.
        let atlas = Atlas::new((1024, 1024), MODEL_CELL);
        Self {
            area,
            atlas,
            prev_snapshot: Vec::new(),
        }
    }

    /// One frame of GUI work: render the launcher into a fresh buffer, snapshot
    /// it, dirty-diff against the previous frame, and atlas-key the changes.
    pub fn paint(&mut self, core: &mut LauncherCore) {
        let mut buf = Buffer::empty(self.area);
        render_core(core, self.area, &mut buf);
        let snapshot = snapshot_buffer(&buf);
        let dirty = dirty_cells(&self.prev_snapshot, &snapshot);
        for &i in &dirty {
            let _ = self.atlas.get_or_insert(snapshot[i].glyph_key());
        }
        self.prev_snapshot = snapshot;
    }

    /// Distinct glyphs resolved into the atlas so far — for tests.
    #[cfg(test)]
    fn atlas_len(&self) -> u32 {
        self.atlas.len()
    }

    /// Visible text currently in the snapshot — for tests.
    #[cfg(test)]
    fn visible_text(&self) -> String {
        self.prev_snapshot.iter().map(|c| c.ch).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_size_fits_whole_cells_and_floors_remainder() {
        let cell = CellMetrics::new(10, 20);
        // 805/10 = 80 cols (5px padding), 615/20 = 30 rows (15px padding).
        assert_eq!(grid_size((805, 615), cell), GridSize { cols: 80, rows: 30 });
    }

    #[test]
    fn grid_size_is_at_least_one_by_one() {
        // A window thinner than a single cell still has a 1×1 grid.
        let cell = CellMetrics::new(10, 20);
        assert_eq!(grid_size((4, 4), cell), GridSize { cols: 1, rows: 1 });
    }

    #[test]
    fn cell_metrics_never_zero() {
        let cell = CellMetrics::new(0, 0);
        assert_eq!(
            cell,
            CellMetrics {
                cell_w: 1,
                cell_h: 1
            }
        );
    }

    #[test]
    fn cell_rect_offsets_by_column_and_row() {
        let cell = CellMetrics::new(10, 20);
        assert_eq!(cell_rect(0, 0, cell), (0, 0, 10, 20));
        assert_eq!(cell_rect(3, 2, cell), (30, 40, 10, 20));
    }

    #[test]
    fn atlas_assigns_a_stable_slot_per_glyph() {
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let a1 = atlas.get_or_insert(GlyphKey::new('a', false)).unwrap();
        assert!(a1.newly_inserted, "first sight allocates");
        let a2 = atlas.get_or_insert(GlyphKey::new('a', false)).unwrap();
        assert!(!a2.newly_inserted, "second sight reuses");
        assert_eq!(a1.index, a2.index, "same glyph keeps its slot");
        assert_eq!(atlas.len(), 1);
    }

    #[test]
    fn atlas_keys_bold_separately_from_regular() {
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let regular = atlas.get_or_insert(GlyphKey::new('x', false)).unwrap();
        let bold = atlas.get_or_insert(GlyphKey::new('x', true)).unwrap();
        assert_ne!(
            regular.index, bold.index,
            "bold is a distinct rasterization"
        );
        assert_eq!(atlas.len(), 2);
    }

    #[test]
    fn atlas_lays_slots_out_row_major() {
        // 64/16 = 4 columns. Slot 0 at (0,0), slot 4 wraps to row 1 at (0,16).
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let slots: Vec<_> = "abcde"
            .chars()
            .map(|c| atlas.get_or_insert(GlyphKey::new(c, false)).unwrap())
            .collect();
        assert_eq!(slots[0].px, (0, 0));
        assert_eq!(slots[3].px, (48, 0));
        assert_eq!(
            slots[4].px,
            (0, 16),
            "5th glyph wraps to the next atlas row"
        );
    }

    #[test]
    fn atlas_returns_none_when_full() {
        // A 2×1-slot atlas holds exactly two distinct glyphs.
        let mut atlas = Atlas::new((32, 16), CellMetrics::new(16, 16));
        assert_eq!(atlas.capacity(), 2);
        assert!(atlas.get_or_insert(GlyphKey::new('a', false)).is_some());
        assert!(atlas.get_or_insert(GlyphKey::new('b', false)).is_some());
        assert!(
            atlas.get_or_insert(GlyphKey::new('c', false)).is_none(),
            "a third distinct glyph overflows the full atlas",
        );
        // A glyph already resident still resolves even when the atlas is full.
        assert!(atlas.get_or_insert(GlyphKey::new('a', false)).is_some());
    }

    #[test]
    fn atlas_uv_rect_is_normalized_within_unit_square() {
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let slot = atlas.get_or_insert(GlyphKey::new('a', false)).unwrap();
        let (u0, v0, u1, v1) = atlas.uv_rect(slot);
        assert_eq!((u0, v0), (0.0, 0.0));
        assert_eq!((u1, v1), (0.25, 0.25), "16/64 = one quarter of the atlas");
        assert!((0.0..=1.0).contains(&u1) && (0.0..=1.0).contains(&v1));
    }

    #[test]
    fn dirty_cells_reports_only_changed_indices() {
        let prev = vec![GridCell::default(); 4];
        let mut cur = prev.clone();
        cur[2].ch = 'Z';
        assert_eq!(dirty_cells(&prev, &cur), vec![2]);
    }

    #[test]
    fn dirty_cells_notices_color_and_weight_changes() {
        let prev = vec![GridCell::default(); 2];
        let mut cur = prev.clone();
        cur[0].fg = 0x00_FF_00; // recolor
        cur[1].bold = true; // re-weight
        assert_eq!(dirty_cells(&prev, &cur), vec![0, 1]);
    }

    #[test]
    fn dirty_cells_is_empty_when_nothing_moved() {
        let prev = vec![GridCell::default(); 8];
        let cur = prev.clone();
        assert!(dirty_cells(&prev, &cur).is_empty());
    }

    #[test]
    fn dirty_cells_repaints_everything_on_resize() {
        let prev = vec![GridCell::default(); 4];
        let cur = vec![GridCell::default(); 6];
        // Length change ⇒ full repaint, all six new cells dirty.
        assert_eq!(dirty_cells(&prev, &cur), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn pack_color_distinguishes_color_kinds() {
        let rgb = pack_color(Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(rgb, 0x12_34_56);
        // A palette / named color must not collide with the RGB that shares its
        // low bits.
        assert_ne!(pack_color(Color::Indexed(0x56)), rgb);
        assert_ne!(pack_color(Color::Red), rgb);
        assert_ne!(pack_color(Color::Red), pack_color(Color::Blue));
    }

    #[test]
    fn gui_model_renders_launcher_and_keys_its_glyphs() {
        use crate::config::Config;

        let area = Rect::new(0, 0, 80, 40);
        let mut core = LauncherCore::from_apps(crate::bench::synthetic_apps(), Config::default());
        let mut model = GuiModel::new(area);

        model.paint(&mut core);

        assert!(
            model.visible_text().contains("Firefox"),
            "the snapshot should hold the rendered launcher frame",
        );
        assert!(
            model.atlas_len() > 0,
            "the renderer should have keyed the frame's glyphs into the atlas",
        );
    }

    #[test]
    fn gui_model_repaint_of_unchanged_frame_keys_no_new_glyphs() {
        use crate::config::Config;

        let area = Rect::new(0, 0, 80, 40);
        let mut core = LauncherCore::from_apps(crate::bench::synthetic_apps(), Config::default());
        let mut model = GuiModel::new(area);

        model.paint(&mut core);
        let after_first = model.atlas_len();
        assert!(after_first > 0);

        // Repainting the same launcher state changes no cells, so the dirty diff
        // is empty and the atlas grows by zero — the steady-state cache-hit path.
        model.paint(&mut core);
        assert_eq!(
            model.atlas_len(),
            after_first,
            "an unchanged repaint must key no new glyphs",
        );
    }
}
