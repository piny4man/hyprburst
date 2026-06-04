//! Shared launcher rendering: paint a [`LauncherCore`] into a ratatui buffer.
//!
//! Factored out of the TUI [`Widget`](ratatui::widgets::Widget) impl so the GPU
//! window and the benchmark probes paint frames through the exact same path the
//! shipped TUI uses. Pushes the layout's column count into the core before
//! projecting its view, so grid navigation stays consistent with what was drawn.

use ratatui::prelude::*;

use crate::domain::launcher_core::{EmptyReason, LauncherCore};
use crate::view::layout::{self, LayoutRects};

/// Render a [`LauncherCore`] into a ratatui buffer.
pub fn render_core(core: &mut LauncherCore, area: Rect, buf: &mut Buffer) {
    let LayoutRects {
        banner: banner_area,
        input: input_area,
        separator: separator_area,
        list: list_area,
        columns,
    } = layout::compute(area, core.config());
    core.set_columns(columns);

    let config = core.config();
    let view = core.view();

    if banner_area.height > 0 && !config.ui.banner.is_empty() {
        let banner_style = Style::new()
            .fg(config.colors.banner)
            .add_modifier(Modifier::BOLD);
        for (i, line) in config
            .ui
            .banner
            .lines()
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
        let sep_line: String = "─".repeat(sep.width as usize);
        buf.set_string(
            sep.x,
            sep.y,
            &sep_line,
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
    let marker_width = marker.chars().count();
    let unselected_prefix: String = " ".repeat(marker_width);

    let columns = columns.max(1);
    let col_width = list_area.width / columns;
    let max_rows = list_area.height as usize;
    let max_cells = max_rows.saturating_mul(columns as usize);

    for (i, entry) in view.entries.iter().enumerate().take(max_cells) {
        let row = (i / columns as usize) as u16;
        let col = (i % columns as usize) as u16;
        if row >= list_area.height {
            break;
        }
        let cell_x = list_area.x + col * col_width;
        let cell_y = list_area.y + row;
        let selected = entry.selected;
        let prefix: &str = if selected { marker } else { &unselected_prefix };
        let style = if selected {
            Style::new()
                .fg(config.colors.selected)
                .bg(config.colors.selected_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };

        let mut line = if config.ui.show_icons {
            format!("{}{} {}", prefix, entry.icon_glyph, entry.name)
        } else {
            format!("{}{}", prefix, entry.name)
        };
        let cell_width = col_width as usize;
        if columns > 1 && line.chars().count() > cell_width {
            line = line.chars().take(cell_width).collect();
        }
        // Pad the selected row to the full cell width so its highlight bar spans
        // the row (the background style fills the trailing spaces too).
        if selected {
            let w = line.chars().count();
            if w < cell_width {
                line.push_str(&" ".repeat(cell_width - w));
            }
        }
        buf.set_string(cell_x, cell_y, &line, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::{Config, UiConfig};
    use crate::domain::desktop::DesktopEntry;

    fn core_with_single_app(config: Config) -> LauncherCore {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        LauncherCore::for_test(vec![app], config)
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
        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut core, area, &mut buf);

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

        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut core, area, &mut buf);

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

        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        render_core(&mut core, area, &mut buf);

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

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_core(&mut core, area, &mut buf);

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

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        render_core(&mut core, area, &mut buf);

        let row = row_at(&buf, 2, area);
        assert!(
            row.contains('▏') && !row.contains('█'),
            "expected custom cursor glyph, got {:?}",
            row
        );
    }
}
