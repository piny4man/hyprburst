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

/// A cell with a non-default background — a solid quad drawn behind the glyphs
/// (e.g. the selection highlight bar, which spans the row including its spaces).
/// Shared by the launcher buffer scan (`window`) and the Rio VT snapshot
/// (`rio`), which both feed the same GL renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BgCell {
    pub col: u16,
    pub row: u16,
    pub color: [f32; 4],
}

/// One non-blank glyph cell to draw, resolved from either the rendered launcher
/// buffer or the Rio terminal grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellInstance {
    pub col: u16,
    pub row: u16,
    pub ch: char,
    pub bold: bool,
    pub color: [f32; 3],
}

/// The atlas tiles that draw `glyphs`, each with the cell it lands in. An icon
/// whose right-hand neighbour is blank and inside the grid spans both cells
/// (left half, right half), like a double-width glyph in a terminal; otherwise
/// every glyph is one tile. `glyphs` must be row-major (both producers are), so
/// the neighbour, if any, is the next entry.
pub fn glyph_tiles(
    glyphs: &[CellInstance],
    cols: u16,
) -> impl Iterator<Item = (CellInstance, GlyphKey)> + '_ {
    glyphs.iter().enumerate().flat_map(move |(i, c)| {
        let next_free = c.col.checked_add(1).is_some_and(|next| next < cols)
            && glyphs
                .get(i + 1)
                .is_none_or(|n| n.row != c.row || n.col != c.col + 1);
        let (first, second) = if next_free && crate::gpu::font::is_icon(c.ch) {
            let right = CellInstance {
                col: c.col + 1,
                ..*c
            };
            (
                (*c, GlyphKey::half(c.ch, GlyphSpan::Left)),
                Some((right, GlyphKey::half(c.ch, GlyphSpan::Right))),
            )
        } else {
            ((*c, GlyphKey::new(c.ch)), None)
        };
        std::iter::once(first).chain(second)
    })
}

/// Map a window's pixel size to a grid: as many whole cells as fit, at least 1×1.
/// The remainder pixels are left as padding the renderer can letterbox. The
/// *total* cell count is capped at `u16::MAX`: the renderer iterates cells
/// linearly in u16 (`build_cells`'s `index as u16`), so `cols × rows` must fit
/// even though each axis alone could clamp to `u16::MAX`.
pub fn grid_size(window_px: (u32, u32), cell: CellMetrics) -> GridSize {
    let cols = (window_px.0 / cell.cell_w).max(1);
    let rows = (window_px.1 / cell.cell_h).max(1);
    // Grids beyond u16 are absurd for a launcher window; clamp rather than
    // wrap so a freak window size can't produce a tiny grid via truncation.
    let cols = cols.min(u16::MAX as u32);
    let rows = rows.min(u16::MAX as u32).min(u16::MAX as u32 / cols);
    GridSize {
        cols: cols as u16,
        rows: rows as u16,
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

/// Identity of a rasterized glyph in the [`Atlas`]. Color does not key the slot
/// (the shader tints a single white-on-clear glyph), and neither does weight:
/// the rasterizer draws one regular-weight bitmap per character, so keying bold
/// separately would just burn a second atlas slot for an identical tile. If
/// true bold rasterization lands later, add the flag back here. The span does
/// key the slot: a two-cell icon is two tiles, distinct from its one-cell form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub ch: char,
    pub span: GlyphSpan,
}

/// Which part of a glyph a one-cell tile holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphSpan {
    /// The whole glyph, fitted to one cell.
    Single,
    /// The left cell of an icon fitted across two cells.
    Left,
    /// The right cell of an icon fitted across two cells.
    Right,
}

impl GlyphKey {
    pub fn new(ch: char) -> Self {
        Self::half(ch, GlyphSpan::Single)
    }

    pub fn half(ch: char, span: GlyphSpan) -> Self {
        Self { ch, span }
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
    fn grid_size_caps_total_cells_within_u16() {
        // A 1px cell on an enormous window: each axis would clamp to u16::MAX,
        // but the product must also fit u16 so linear index math can't wrap.
        let grid = grid_size((u32::MAX, u32::MAX), CellMetrics::new(1, 1));
        assert_eq!(grid.cols, u16::MAX);
        assert!(grid.cols as u32 * grid.rows as u32 <= u16::MAX as u32);
        assert_eq!(grid.cols as u32 * grid.rows as u32, u16::MAX as u32);
    }

    #[test]
    fn grid_size_shrinks_rows_when_product_would_overflow_u16() {
        // 1000×1000 cells exceeds u16; rows shrink to keep the total bounded.
        let grid = grid_size((2000, 2000), CellMetrics::new(2, 2));
        assert_eq!(grid.cols, 1000);
        assert_eq!(u32::from(grid.rows), u16::MAX as u32 / 1000);
        assert!(grid.cols as u32 * grid.rows as u32 <= u16::MAX as u32);
    }

    #[test]
    fn grid_size_clamp_never_yields_zero_cells() {
        let cell = CellMetrics::new(1, 1);
        for (w, h) in [(u32::MAX, 1), (1, u32::MAX)] {
            let grid = grid_size((w, h), cell);
            assert!(grid.cols >= 1 && grid.rows >= 1);
        }
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
        let a1 = atlas.get_or_insert(GlyphKey::new('a')).unwrap();
        assert!(a1.newly_inserted, "first sight allocates");
        let a2 = atlas.get_or_insert(GlyphKey::new('a')).unwrap();
        assert!(!a2.newly_inserted, "second sight reuses");
        assert_eq!(a1.index, a2.index, "same glyph keeps its slot");
        assert_eq!(atlas.len(), 1);
    }

    #[test]
    fn atlas_keys_bold_and_regular_to_one_slot() {
        // Weight is not part of the key: the rasterizer draws one bitmap per
        // character, so a bold cell reuses the regular tile instead of doubling
        // atlas consumption.
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let regular = atlas.get_or_insert(GlyphKey::new('x')).unwrap();
        let bold = atlas.get_or_insert(GlyphKey::new('x')).unwrap();
        assert_eq!(
            regular.index, bold.index,
            "bold and regular share the character's slot"
        );
        assert_eq!(atlas.len(), 1);
    }

    #[test]
    fn atlas_keys_each_half_of_a_wide_icon_to_its_own_slot() {
        // A two-cell icon is drawn as two tiles; neither half may alias the
        // one-cell rendering of the same character.
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let icon = '\u{f269}';
        let single = atlas.get_or_insert(GlyphKey::new(icon)).unwrap();
        let left = atlas
            .get_or_insert(GlyphKey::half(icon, GlyphSpan::Left))
            .unwrap();
        let right = atlas
            .get_or_insert(GlyphKey::half(icon, GlyphSpan::Right))
            .unwrap();
        assert!(left.newly_inserted && right.newly_inserted);
        let mut indices = [single.index, left.index, right.index];
        indices.sort_unstable();
        assert_eq!(indices, [0, 1, 2]);
        assert_eq!(GlyphKey::new(icon).span, GlyphSpan::Single);
    }

    fn glyph(col: u16, row: u16, ch: char) -> CellInstance {
        CellInstance {
            col,
            row,
            ch,
            bold: false,
            color: [1.0; 3],
        }
    }

    fn tiles(glyphs: &[CellInstance], cols: u16) -> Vec<(u16, u16, GlyphKey)> {
        glyph_tiles(glyphs, cols)
            .map(|(c, key)| (c.col, c.row, key))
            .collect()
    }

    const ICON: char = '\u{f269}';

    #[test]
    fn icon_before_a_blank_cell_spans_two_cells() {
        // "> <icon> Firefox": the space after the icon is not a glyph.
        let glyphs = [glyph(0, 0, '>'), glyph(2, 0, ICON), glyph(4, 0, 'F')];
        assert_eq!(
            tiles(&glyphs, 10),
            vec![
                (0, 0, GlyphKey::new('>')),
                (2, 0, GlyphKey::half(ICON, GlyphSpan::Left)),
                (3, 0, GlyphKey::half(ICON, GlyphSpan::Right)),
                (4, 0, GlyphKey::new('F')),
            ]
        );
    }

    #[test]
    fn icon_with_an_occupied_neighbour_or_at_the_edge_stays_one_cell() {
        let crowded = [glyph(0, 0, ICON), glyph(1, 0, 'x')];
        assert_eq!(
            tiles(&crowded, 10),
            vec![(0, 0, GlyphKey::new(ICON)), (1, 0, GlyphKey::new('x'))]
        );
        let edge = [glyph(9, 0, ICON)];
        assert_eq!(tiles(&edge, 10), vec![(9, 0, GlyphKey::new(ICON))]);
    }

    #[test]
    fn icon_spans_when_the_next_glyph_is_on_the_next_row() {
        let glyphs = [glyph(3, 0, ICON), glyph(4, 1, 'x')];
        assert_eq!(
            tiles(&glyphs, 10),
            vec![
                (3, 0, GlyphKey::half(ICON, GlyphSpan::Left)),
                (4, 0, GlyphKey::half(ICON, GlyphSpan::Right)),
                (4, 1, GlyphKey::new('x')),
            ]
        );
    }

    #[test]
    fn text_is_never_widened() {
        let glyphs = [glyph(0, 0, 'a'), glyph(2, 0, 'b')];
        assert_eq!(
            tiles(&glyphs, 10),
            vec![(0, 0, GlyphKey::new('a')), (2, 0, GlyphKey::new('b'))]
        );
    }

    #[test]
    fn atlas_lays_slots_out_row_major() {
        // 64/16 = 4 columns. Slot 0 at (0,0), slot 4 wraps to row 1 at (0,16).
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let slots: Vec<_> = "abcde"
            .chars()
            .map(|c| atlas.get_or_insert(GlyphKey::new(c)).unwrap())
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
        assert!(atlas.get_or_insert(GlyphKey::new('a')).is_some());
        assert!(atlas.get_or_insert(GlyphKey::new('b')).is_some());
        assert!(
            atlas.get_or_insert(GlyphKey::new('c')).is_none(),
            "a third distinct glyph overflows the full atlas",
        );
        // A glyph already resident still resolves even when the atlas is full.
        assert!(atlas.get_or_insert(GlyphKey::new('a')).is_some());
    }

    #[test]
    fn atlas_uv_rect_is_normalized_within_unit_square() {
        let mut atlas = Atlas::new((64, 64), CellMetrics::new(16, 16));
        let slot = atlas.get_or_insert(GlyphKey::new('a')).unwrap();
        let (u0, v0, u1, v1) = atlas.uv_rect(slot);
        assert_eq!((u0, v0), (0.0, 0.0));
        assert_eq!((u1, v1), (0.25, 0.25), "16/64 = one quarter of the atlas");
        assert!((0.0..=1.0).contains(&u1) && (0.0..=1.0).contains(&v1));
    }
}
