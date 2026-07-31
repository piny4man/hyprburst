//! Cell-grid bookkeeping for the GPU-rendered launcher window.
//!
//! The windowed launcher ([`crate::gpu::window`]) owns its rendering surface
//! *without* a terminal emulator: a winit window, a hand-written OpenGL cell
//! renderer (glutin context + glow draw + an `ab_glyph` glyph atlas), and an
//! in-process [`LauncherCore`](crate::domain::launcher_core::LauncherCore). The
//! launcher is painted with [`render_core`](crate::view::render::render_core)
//! straight into a ratatui [`Buffer`](ratatui::buffer::Buffer) (which *is* a cell
//! grid), and that buffer is drawn directly.
//!
//! A cell renderer is mostly bookkeeping that needs no GPU and no window, so that
//! bookkeeping lives here where it can be unit-tested:
//!
//! - [`CellMetrics`] / [`grid_size`] / [`cell_rect`] — map a window's pixel size
//!   and a monospace cell size to a grid and each cell's pixel rect.
//! - [`Atlas`] — keys rasterized glyphs into a fixed texture grid: each distinct
//!   [`GlyphKey`] is rasterized and uploaded at most once, then reused.

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

/// Map a physical window position to a whole terminal cell.
pub fn cell_at_pixel(
    position: (f64, f64),
    cell: CellMetrics,
    grid: GridSize,
) -> Option<(u16, u16)> {
    let (x, y) = position;
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return None;
    }

    let col = (x / cell.cell_w as f64).floor();
    let row = (y / cell.cell_h as f64).floor();
    if col >= grid.cols as f64 || row >= grid.rows as f64 {
        return None;
    }

    Some((col as u16, row as u16))
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
/// every tile is taken, further new glyphs return `None` (the launcher's app set
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
    fn pixel_position_maps_to_whole_cell() {
        let cell = CellMetrics::new(10, 20);
        let grid = GridSize { cols: 3, rows: 2 };

        assert_eq!(cell_at_pixel((19.9, 20.0), cell, grid), Some((1, 1)));
        assert_eq!(cell_at_pixel((30.0, 0.0), cell, grid), None);
        assert_eq!(cell_at_pixel((-1.0, 0.0), cell, grid), None);
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
}
