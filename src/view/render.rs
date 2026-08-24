//! Shared launcher rendering: paint a [`LauncherCore`] into a ratatui buffer.
//!
//! Factored out of the TUI [`Widget`](ratatui::widgets::Widget) impl so the GPU
//! window and the benchmark probes paint frames through the exact same path the
//! shipped TUI uses. Pushes the layout's column count into the core before
//! projecting its view, so grid navigation stays consistent with what was drawn.

use ratatui::prelude::*;

use crate::domain::config::Config;
use crate::domain::launcher_core::{EmptyReason, LauncherCore};
use crate::view::layout::{self, LayoutRects};

/// Geometry and config-derived strings recomputed only when the terminal
/// geometry changes. Steady-state frames paint from this cache without a single
/// heap allocation beyond the per-cell label buffer (reused, capacity retained).
pub struct RenderCache {
    key: (u16, u16, u16, u16),
    rects: LayoutRects,
    /// The banner split into owned lines (`config.ui.banner.lines()`), so the
    /// frame path never re-splits or re-counts it.
    banner_lines: Vec<String>,
    /// `"─" * separator.width`.
    sep_line: String,
    /// Spaces matching the selected-marker width (unselected row prefix).
    unselected_prefix: String,
    /// One cell-width of spaces, sliced down for selection-bar padding.
    padding_spaces: String,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            key: (u16::MAX, u16::MAX, 0, 0),
            rects: LayoutRects {
                banner: Rect::default(),
                input: Rect::default(),
                separator: None,
                list: Rect::default(),
                columns: 1,
            },
            banner_lines: Vec::new(),
            sep_line: String::new(),
            unselected_prefix: String::new(),
            padding_spaces: String::new(),
        }
    }

    /// Recompute everything derived from `(area, config)` — config values are
    /// fixed for the process's lifetime, so the geometry alone keys the cache.
    fn refresh(&mut self, area: Rect, config: &Config) {
        let key = (area.x, area.y, area.width, area.height);
        if self.key == key {
            return;
        }
        self.key = key;
        self.rects = layout::compute(area, config);
        self.unselected_prefix = " ".repeat(config.ui.selected_marker.chars().count());
        self.sep_line = "─".repeat(self.rects.separator.map_or(0, |r| r.width as usize));
        let cell_width = (self.rects.list.width / self.rects.columns.max(1)) as usize;
        self.padding_spaces = " ".repeat(cell_width);
        self.banner_lines = config.ui.banner.lines().map(str::to_string).collect();
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a [`LauncherCore`] into a ratatui buffer, reusing `cache` across
/// frames. Pushes the layout's column count into the core before projecting its
/// view, so grid navigation stays consistent with what was drawn.
pub fn render_core(cache: &mut RenderCache, core: &mut LauncherCore, area: Rect, buf: &mut Buffer) {
    cache.refresh(area, core.config());
    core.set_columns(cache.rects.columns);

    let config = core.config();
    let view = core.view();
    let RenderCache {
        rects,
        banner_lines,
        sep_line,
        unselected_prefix,
        padding_spaces,
        ..
    } = cache;
    let banner_area = rects.banner;
    let input_area = rects.input;
    let separator_area = rects.separator;
    let list_area = rects.list;
    let columns = rects.columns;

    if banner_area.height > 0 && config.ui.show_banner && !config.ui.banner.is_empty() {
        let banner_style = Style::new()
            .fg(config.colors.banner)
            .add_modifier(Modifier::BOLD);
        for (i, line) in banner_lines
            .iter()
            .take(banner_area.height as usize)
            .enumerate()
        {
            buf.set_string(banner_area.x, banner_area.y + i as u16, line, banner_style);
        }
    }

    if input_area.height == 0 {
        return;
    }

    let prompt_text = format!("{}{}", config.ui.prompt, view.query);
    let input_style = Style::new()
        .fg(config.colors.prompt)
        .add_modifier(Modifier::BOLD);
    buf.set_string(input_area.x, input_area.y, &prompt_text, input_style);

    if config.ui.show_cursor {
        let cursor_x = input_area.x + prompt_text.chars().count() as u16;
        if cursor_x < input_area.x + input_area.width {
            buf.set_string(
                cursor_x,
                input_area.y,
                &config.ui.cursor_char,
                Style::new().fg(config.colors.prompt),
            );
        }
    }

    if let Some(sep) = separator_area {
        buf.set_string(
            sep.x,
            sep.y,
            sep_line,
            Style::new().fg(config.colors.prompt),
        );
    }

    if list_area.height == 0 {
        return;
    }

    if view.entries.is_empty() {
        let msg = match view.empty_reason {
            Some(EmptyReason::NoMatches) => "No matches",
            _ => "No applications found",
        };
        let style = Style::new().fg(config.colors.empty);
        buf.set_string(list_area.x, list_area.y, msg, style);
        return;
    }

    let marker = config.ui.selected_marker.as_str();
    let columns = columns.max(1);
    let col_width = list_area.width / columns;
    let max_rows = list_area.height as usize;
    let max_cells = max_rows.saturating_mul(columns as usize);
    let cell_width = col_width as usize;

    // Reused label buffer: one allocation until it outgrows its capacity.
    let mut line_buf = String::with_capacity(cell_width + 4);

    for (i, entry) in view.entries.iter().enumerate().take(max_cells) {
        let row = (i / columns as usize) as u16;
        let col = (i % columns as usize) as u16;
        if row >= list_area.height {
            break;
        }
        let cell_x = list_area.x + col * col_width;
        let cell_y = list_area.y + row;
        let selected = entry.selected;
        let prefix: &str = if selected { marker } else { unselected_prefix };
        let style = if selected {
            Style::new()
                .fg(config.colors.selected)
                .bg(config.colors.selected_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };

        line_buf.clear();
        line_buf.push_str(prefix);
        if config.ui.show_icons {
            line_buf.push_str(entry.icon_glyph);
            line_buf.push(' ');
        }
        line_buf.push_str(entry.name);

        if columns > 1 && line_buf.chars().count() > cell_width {
            // Truncate at a char boundary without reallocating.
            let keep = line_buf
                .char_indices()
                .nth(cell_width)
                .map_or(line_buf.len(), |(byte_idx, _)| byte_idx);
            line_buf.truncate(keep);
        }
        // Pad the selected row to the full cell width so its highlight bar spans
        // the row (the background style fills the trailing spaces too).
        if selected {
            let w = line_buf.chars().count();
            if w < cell_width {
                line_buf.push_str(&padding_spaces[..cell_width - w]);
            }
        }
        buf.set_string(cell_x, cell_y, &line_buf, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::{Config, UiConfig};
    use crate::domain::desktop::DesktopEntry;

    fn core_with_single_app(config: Config) -> LauncherCore {
        let app = DesktopEntry::new("firefox", "Firefox", "firefox", "firefox");
        LauncherCore::for_test(vec![app], config)
    }

    fn cache() -> RenderCache {
        RenderCache::new()
    }

    fn row_at(buf: &Buffer, y: u16, area: Rect) -> String {
        (0..area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn render_draws_icon_before_app_name() {
        let cfg = Config {
            ui: UiConfig {
                banner: String::new(),
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let mut core = core_with_single_app(cfg);
        let mut cache = cache();
        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut cache, &mut core, area, &mut buf);

        let row = row_at(&buf, 3, area);
        assert!(
            row.contains("\u{f269}") && row.contains("Firefox"),
            "expected nerd font glyph and name on row, got {:?}",
            row
        );
    }

    #[test]
    fn render_hides_icons_when_show_icons_false() {
        let cfg = Config {
            ui: UiConfig {
                banner: String::new(),
                show_icons: false,
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let mut core = core_with_single_app(cfg);
        let mut cache = cache();

        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut cache, &mut core, area, &mut buf);

        let row = row_at(&buf, 3, area);
        assert!(
            !row.contains('\u{f269}') && row.contains("Firefox"),
            "icon glyph should be suppressed, got {:?}",
            row
        );
    }

    #[test]
    fn render_uses_custom_selected_marker() {
        let cfg = Config {
            ui: UiConfig {
                banner: String::new(),
                selected_marker: "» ".into(),
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let mut core = core_with_single_app(cfg);
        let mut cache = cache();

        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut cache, &mut core, area, &mut buf);

        let row = row_at(&buf, 3, area);
        assert!(
            row.contains("» ") && row.contains("Firefox"),
            "expected custom marker on selected row, got {:?}",
            row
        );
    }

    #[test]
    fn render_hides_cursor_when_show_cursor_false() {
        let cfg = Config {
            ui: UiConfig {
                banner: String::new(),
                show_cursor: false,
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let mut core = core_with_single_app(cfg);
        let mut cache = cache();

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_core(&mut cache, &mut core, area, &mut buf);

        let row = row_at(&buf, 2, area);
        assert!(
            !row.contains('█'),
            "cursor glyph should be suppressed, got {:?}",
            row
        );
    }

    #[test]
    fn render_uses_custom_cursor_char() {
        let cfg = Config {
            ui: UiConfig {
                banner: String::new(),
                cursor_char: "▏".into(),
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let mut core = core_with_single_app(cfg);
        let mut cache = cache();

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_core(&mut cache, &mut core, area, &mut buf);

        let row = row_at(&buf, 2, area);
        assert!(
            row.contains('▏') && !row.contains('█'),
            "expected custom cursor glyph, got {:?}",
            row
        );
    }
}
