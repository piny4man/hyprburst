use std::collections::HashMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;

use crate::config::Config;
use crate::desktop::{DesktopEntry, discover_apps};
use crate::history::{History, score as history_score};
use crate::icon::fallback_glyph;
use crate::layout::{self, LayoutRects};
use crate::search::filter_and_rank;

pub struct Launcher {
    pub apps: Vec<DesktopEntry>,
    pub query: String,
    pub filtered: Vec<(usize, u32)>,
    pub selected_index: usize,
    pub running: bool,
    pub(crate) history: Option<History>,
    pub(crate) scores: HashMap<String, f64>,
    pub(crate) config: Config,
    pub(crate) last_columns: u16,
}

impl Launcher {
    pub fn new(config: Config) -> Self {
        let apps = discover_apps();
        let running = !apps.is_empty();
        let history = History::open().ok();
        let scores = history.as_ref().map(load_scores).unwrap_or_default();
        let mut launcher = Self {
            apps,
            query: String::new(),
            filtered: Vec::new(),
            selected_index: 0,
            running,
            history,
            scores,
            config,
            last_columns: 1,
        };
        launcher.rebuild_filtered();
        launcher
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if !self.running {
            return;
        }

        match code {
            KeyCode::Esc => self.running = false,
            KeyCode::Tab => self.autocomplete(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::Down => self.move_vertical(1),
            KeyCode::Left => self.move_horizontal(-1),
            KeyCode::Right => self.move_horizontal(1),
            KeyCode::Enter => self.launch_selected(),
            KeyCode::Backspace => {
                self.query.pop();
                self.rebuild_filtered();
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.rebuild_filtered();
            }
            _ => {}
        }
    }

    fn rebuild_filtered(&mut self) {
        let ranked = filter_and_rank(&self.query, &self.apps, &self.scores);
        self.filtered = ranked
            .into_iter()
            .map(|(app, score)| {
                let idx = self.apps.iter().position(|a| std::ptr::eq(a, app)).unwrap();
                (idx, score)
            })
            .collect();
        self.selected_index = 0;
    }

    fn autocomplete(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let (idx, _) = self.filtered[self.selected_index.min(self.filtered.len() - 1)];
        let name = &self.apps[idx].name;
        self.query = name.to_string();
        self.rebuild_filtered();
    }

    fn move_vertical(&mut self, dr: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let cols = self.last_columns.max(1) as i32;
        let n = self.filtered.len() as i32;
        let idx = self.selected_index as i32;
        let col = idx % cols;
        let rows_in_col = (n - 1 - col).div_euclid(cols) + 1;
        let row_in_col = idx.div_euclid(cols);
        let new_row = (row_in_col + dr).rem_euclid(rows_in_col);
        self.selected_index = (new_row * cols + col) as usize;
    }

    fn move_horizontal(&mut self, dc: i32) {
        if self.filtered.is_empty() || self.last_columns <= 1 {
            return;
        }
        let cols = self.last_columns as i32;
        let n = self.filtered.len() as i32;
        let idx = self.selected_index as i32;
        let row = idx.div_euclid(cols);
        let col = idx % cols;
        let last_col_in_row = (n - 1 - row * cols).min(cols - 1);
        let new_col = (col + dc).rem_euclid(last_col_in_row + 1);
        self.selected_index = (row * cols + new_col) as usize;
    }

    fn page_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(self.config.ui.page_size);
    }

    fn page_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected_index =
            (self.selected_index + self.config.ui.page_size).min(self.filtered.len() - 1);
    }

    fn launch_selected(&mut self) {
        if self.selected_index < self.filtered.len() {
            let (idx, _) = self.filtered[self.selected_index];
            let app = &self.apps[idx];
            if !app.exec.is_empty() {
                let _ = Command::new("hyprctl")
                    .args(["dispatch", "exec", "--", &app.exec])
                    .spawn();
            }
            if let Some(history) = &self.history {
                let _ = history.record_launch(&app.id, &app.name);
            }
        }
        self.running = false;
    }
}

pub(crate) fn icon_glyph_for(app: &DesktopEntry) -> &'static str {
    fallback_glyph(&app.icon, &app.name)
}

fn load_scores(history: &History) -> HashMap<String, f64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    history
        .all()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.desktop_id.clone(), history_score(&entry, now)))
        .collect()
}

impl Widget for &mut Launcher {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let LayoutRects {
            banner: banner_area,
            input: input_area,
            separator: separator_area,
            list: list_area,
            columns,
        } = layout::compute(area, &self.config);
        self.last_columns = columns.max(1);

        if banner_area.height > 0 && !self.config.ui.banner.is_empty() {
            let banner_style = Style::new()
                .fg(self.config.colors.banner)
                .add_modifier(Modifier::BOLD);
            for (i, line) in self
                .config
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

        let prompt_text = format!("{}{}", self.config.ui.prompt, self.query);
        let input_style = Style::new()
            .fg(self.config.colors.prompt)
            .add_modifier(Modifier::BOLD);
        buf.set_string(input_area.x, input_area.y, &prompt_text, input_style);

        if self.config.ui.show_cursor {
            let cursor_x = input_area.x + prompt_text.chars().count() as u16;
            if cursor_x < input_area.x + input_area.width {
                buf.set_string(
                    cursor_x,
                    input_area.y,
                    &self.config.ui.cursor_char,
                    Style::new().fg(self.config.colors.prompt),
                );
            }
        }

        if let Some(sep) = separator_area {
            let sep_line: String = "─".repeat(sep.width as usize);
            buf.set_string(
                sep.x,
                sep.y,
                &sep_line,
                Style::new().fg(self.config.colors.prompt),
            );
        }

        if list_area.height == 0 {
            return;
        }

        if self.filtered.is_empty() {
            let msg = if self.query.is_empty() {
                "No applications found"
            } else {
                "No matches"
            };
            let style = Style::new().fg(self.config.colors.empty);
            buf.set_string(list_area.x, list_area.y, msg, style);
            return;
        }

        let marker = self.config.ui.selected_marker.as_str();
        let marker_width = marker.chars().count();
        let unselected_prefix: String = " ".repeat(marker_width);

        let columns = columns.max(1);
        let col_width = list_area.width / columns;
        let max_rows = list_area.height as usize;
        let max_cells = max_rows.saturating_mul(columns as usize);

        for (i, &(idx, _score)) in self.filtered.iter().enumerate().take(max_cells) {
            let row = (i / columns as usize) as u16;
            let col = (i % columns as usize) as u16;
            if row >= list_area.height {
                break;
            }
            let cell_x = list_area.x + col * col_width;
            let cell_y = list_area.y + row;
            let selected = i == self.selected_index;
            let prefix: &str = if selected { marker } else { &unselected_prefix };
            let style = if selected {
                Style::new()
                    .fg(self.config.colors.selected)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };

            let app = &self.apps[idx];
            let mut line = if self.config.ui.show_icons {
                let glyph = icon_glyph_for(app);
                format!("{}{} {}", prefix, glyph, app.name)
            } else {
                format!("{}{}", prefix, app.name)
            };
            if columns > 1 {
                let cell_width = col_width as usize;
                let line_width = line.chars().count();
                if line_width > cell_width {
                    line = line.chars().take(cell_width).collect();
                }
            }
            buf.set_string(cell_x, cell_y, &line, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UiConfig;

    fn make_launcher_with_apps() -> Launcher {
        Launcher {
            apps: vec![
                DesktopEntry {
                    id: "app-a".into(),
                    name: "App A".into(),
                    icon: "icon-a".into(),
                    exec: "app-a".into(),
                },
                DesktopEntry {
                    id: "app-b".into(),
                    name: "App B".into(),
                    icon: "icon-b".into(),
                    exec: "app-b".into(),
                },
                DesktopEntry {
                    id: "app-c".into(),
                    name: "App C".into(),
                    icon: "icon-c".into(),
                    exec: "app-c".into(),
                },
            ],
            query: String::new(),
            filtered: vec![(0, 0), (1, 0), (2, 0)],
            selected_index: 0,
            running: true,
            history: None,
            scores: std::collections::HashMap::new(),
            config: Config::default(),
            last_columns: 1,
        }
    }

    #[test]
    fn down_key_moves_selection_down() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Down);
        assert_eq!(launcher.selected_index, 1);
    }

    #[test]
    fn up_key_moves_selection_up() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 1;
        launcher.handle_key(KeyCode::Up);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn down_wraps_to_first() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 2;
        launcher.handle_key(KeyCode::Down);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn up_wraps_to_last() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 0;
        launcher.handle_key(KeyCode::Up);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn enter_stops_launcher() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Enter);
        assert!(!launcher.running);
    }

    #[test]
    fn escape_stops_launcher() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Esc);
        assert!(!launcher.running);
    }

    #[test]
    fn typing_filters_results() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Char('a'));
        assert_eq!(launcher.query, "a");
        assert!(!launcher.filtered.is_empty());
    }

    #[test]
    fn backspace_removes_char() {
        let mut launcher = make_launcher_with_apps();
        launcher.query = "a".into();
        launcher.handle_key(KeyCode::Backspace);
        assert_eq!(launcher.query, "");
    }

    #[test]
    fn tab_autocompletes_top_match() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Char('a'));
        launcher.handle_key(KeyCode::Tab);
        assert!(!launcher.query.is_empty());
    }

    #[test]
    fn page_down_moves_selection_down() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::PageDown);
        assert_eq!(launcher.selected_index, 10.min(launcher.filtered.len() - 1));
    }

    #[test]
    fn page_up_moves_selection_up() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 15;
        launcher.handle_key(KeyCode::PageUp);
        assert_eq!(launcher.selected_index, 5);
    }

    #[test]
    fn empty_apps_list_stops_on_escape() {
        let mut launcher = Launcher {
            apps: vec![],
            query: String::new(),
            filtered: vec![],
            selected_index: 0,
            running: true,
            history: None,
            scores: std::collections::HashMap::new(),
            config: Config::default(),
            last_columns: 1,
        };
        launcher.handle_key(KeyCode::Esc);
        assert!(!launcher.running);
    }

    #[test]
    fn search_rebuilds_filtered_on_typing() {
        let mut launcher = make_launcher_with_apps();
        launcher.handle_key(KeyCode::Char('A'));
        assert_eq!(launcher.query, "A");
        assert!(launcher.selected_index == 0);
    }

    #[test]
    fn enter_records_launch_in_history() {
        let history = History::in_memory().unwrap();
        let mut launcher = make_launcher_with_apps();
        launcher.history = Some(history);

        launcher.handle_key(KeyCode::Enter);

        let entry = launcher
            .history
            .as_ref()
            .unwrap()
            .get("app-a")
            .unwrap()
            .unwrap();
        assert_eq!(entry.launch_count, 1);
        assert_eq!(entry.app_name, "App A");
    }

    #[test]
    fn icon_glyph_maps_known_app_to_nerd_font() {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        assert_eq!(icon_glyph_for(&app), "\u{f269}");
    }

    #[test]
    fn icon_glyph_unknown_app_returns_generic() {
        let app = DesktopEntry {
            id: "xyz".into(),
            name: "Qwerty".into(),
            icon: "zzz".into(),
            exec: "qwerty".into(),
        };
        assert_eq!(icon_glyph_for(&app), "\u{f1b2}");
    }

    #[test]
    fn render_draws_icon_before_app_name() {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        let mut launcher = Launcher {
            apps: vec![app],
            query: String::new(),
            filtered: vec![(0, 0)],
            selected_index: 0,
            running: true,
            history: None,
            scores: std::collections::HashMap::new(),
            config: Config {
                ui: UiConfig {
                    banner: String::new(),
                    ..UiConfig::default()
                },
                ..Config::default()
            },
            last_columns: 1,
        };
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        (&mut launcher).render(area, &mut buf);

        let row = (0..area.width)
            .map(|x| buf[(x, 2)].symbol().to_string())
            .collect::<String>();
        assert!(
            row.contains("\u{f269}") && row.contains("Firefox"),
            "expected nerd font glyph and name on row, got {:?}",
            row
        );
    }

    fn launcher_with_single_app(config: Config) -> Launcher {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        Launcher {
            apps: vec![app],
            query: String::new(),
            filtered: vec![(0, 0)],
            selected_index: 0,
            running: true,
            history: None,
            scores: std::collections::HashMap::new(),
            config,
            last_columns: 1,
        }
    }

    fn row_at(buf: &Buffer, y: u16, area: Rect) -> String {
        (0..area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
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
        let mut launcher = launcher_with_single_app(cfg);

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        (&mut launcher).render(area, &mut buf);

        let row = row_at(&buf, 2, area);
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
        let mut launcher = launcher_with_single_app(cfg);

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        (&mut launcher).render(area, &mut buf);

        let row = row_at(&buf, 2, area);
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
        let mut launcher = launcher_with_single_app(cfg);

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        (&mut launcher).render(area, &mut buf);

        let row = row_at(&buf, 1, area);
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
        let mut launcher = launcher_with_single_app(cfg);

        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        (&mut launcher).render(area, &mut buf);

        let row = row_at(&buf, 1, area);
        assert!(
            row.contains('▏') && !row.contains('█'),
            "expected custom cursor glyph, got {:?}",
            row
        );
    }

    fn make_launcher_with_n_apps(n: usize) -> Launcher {
        let apps: Vec<DesktopEntry> = (0..n)
            .map(|i| DesktopEntry {
                id: format!("app-{}", i),
                name: format!("App {}", i),
                icon: format!("icon-{}", i),
                exec: format!("app-{}", i),
            })
            .collect();
        let filtered = (0..n).map(|i| (i, 0u32)).collect();
        Launcher {
            apps,
            query: String::new(),
            filtered,
            selected_index: 0,
            running: true,
            history: None,
            scores: std::collections::HashMap::new(),
            config: Config::default(),
            last_columns: 1,
        }
    }

    #[test]
    fn grid_down_moves_to_next_row_same_column() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 1;
        launcher.handle_key(KeyCode::Down);
        assert_eq!(launcher.selected_index, 5);
    }

    #[test]
    fn grid_up_moves_to_prev_row_same_column() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 6;
        launcher.handle_key(KeyCode::Up);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn grid_right_moves_within_row() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 1;
        launcher.handle_key(KeyCode::Right);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn grid_left_moves_within_row() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 2;
        launcher.handle_key(KeyCode::Left);
        assert_eq!(launcher.selected_index, 1);
    }

    #[test]
    fn grid_right_wraps_within_row() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 3;
        launcher.handle_key(KeyCode::Right);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn grid_left_wraps_within_row() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 4;
        launcher.handle_key(KeyCode::Left);
        assert_eq!(launcher.selected_index, 7);
    }

    #[test]
    fn grid_down_wraps_to_top_same_column() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 9;
        launcher.handle_key(KeyCode::Down);
        assert_eq!(launcher.selected_index, 1);
    }

    #[test]
    fn grid_up_wraps_to_bottom_same_column() {
        let mut launcher = make_launcher_with_n_apps(12);
        launcher.last_columns = 4;
        launcher.selected_index = 2;
        launcher.handle_key(KeyCode::Up);
        assert_eq!(launcher.selected_index, 10);
    }

    #[test]
    fn grid_down_into_missing_last_row_cell_wraps_to_top() {
        // 7 apps, 3 cols → rows: [0,1,2] [3,4,5] [6]. Col 2 only exists on
        // rows 0 and 1. Down from idx 5 (row 1 col 2) wraps to idx 2 (row 0
        // col 2) instead of landing on a non-existent row-2 col-2 cell.
        let mut launcher = make_launcher_with_n_apps(7);
        launcher.last_columns = 3;
        launcher.selected_index = 5;
        launcher.handle_key(KeyCode::Down);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn grid_right_on_short_last_row_wraps_within_that_row() {
        // 7 apps, 3 cols → last row has only idx 6. Right from 6 wraps to 6
        // itself (single-cell row).
        let mut launcher = make_launcher_with_n_apps(7);
        launcher.last_columns = 3;
        launcher.selected_index = 6;
        launcher.handle_key(KeyCode::Right);
        assert_eq!(launcher.selected_index, 6);
    }

    #[test]
    fn list_mode_left_right_is_noop() {
        let mut launcher = make_launcher_with_n_apps(5);
        launcher.last_columns = 1;
        launcher.selected_index = 2;
        launcher.handle_key(KeyCode::Left);
        assert_eq!(launcher.selected_index, 2);
        launcher.handle_key(KeyCode::Right);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn empty_query_places_most_used_first() {
        let mut launcher = make_launcher_with_apps();
        launcher.scores.insert("app-c".to_string(), 42.0);
        launcher.rebuild_filtered();

        let (top_idx, _) = launcher.filtered[0];
        assert_eq!(launcher.apps[top_idx].id, "app-c");
    }
}
