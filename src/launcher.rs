use std::collections::HashMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::prelude::*;

use crate::desktop::{DesktopEntry, discover_apps};
use crate::history::{History, score as history_score};
use crate::search::filter_and_rank;

pub struct Launcher {
    pub apps: Vec<DesktopEntry>,
    pub query: String,
    pub filtered: Vec<(usize, u32)>,
    pub selected_index: usize,
    pub running: bool,
    pub(crate) history: Option<History>,
    pub(crate) scores: HashMap<String, f64>,
}

impl Launcher {
    pub fn new() -> Self {
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
        };
        launcher.rebuild_filtered();
        launcher
    }

    pub fn handle_event(&mut self, event: &crossterm::event::Event) {
        if !self.running {
            return;
        }

        if crate::input::is_escape(event) {
            self.running = false;
            return;
        }

        if crate::input::is_tab(event) {
            self.autocomplete();
            return;
        }

        if crate::input::is_page_up(event) {
            self.page_up();
            return;
        }

        if crate::input::is_page_down(event) {
            self.page_down();
            return;
        }

        if crate::input::is_up(event) {
            if self.selected_index == 0 {
                self.selected_index = self.filtered.len().saturating_sub(1);
            } else {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            return;
        }

        if crate::input::is_down(event) {
            if self.selected_index + 1 >= self.filtered.len() {
                self.selected_index = 0;
            } else {
                self.selected_index += 1;
            }
            return;
        }

        if crate::input::is_enter(event) {
            self.launch_selected();
            return;
        }

        if crate::input::is_backspace(event) {
            self.query.pop();
            self.rebuild_filtered();
            return;
        }

        if let Some(c) = crate::input::char_from_event(event) {
            self.query.push(c);
            self.rebuild_filtered();
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

    fn page_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let page_size = 10;
        self.selected_index = self.selected_index.saturating_sub(page_size);
    }

    fn page_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let page_size = 10;
        self.selected_index = (self.selected_index + page_size).min(self.filtered.len() - 1);
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
        let input_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };

        let prompt = format!("> {}", self.query);
        let input_style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        buf.set_string(input_area.x, input_area.y, &prompt, input_style);

        let cursor_x = input_area.x + 2 + self.query.len() as u16;
        if cursor_x < input_area.x + input_area.width {
            buf.set_string(cursor_x, input_area.y, "█", Style::new().fg(Color::Cyan));
        }

        let list_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };

        if self.filtered.is_empty() {
            let msg = if self.query.is_empty() {
                "No applications found"
            } else {
                "No matches"
            };
            let style = Style::new().fg(Color::Yellow);
            buf.set_string(list_area.x, list_area.y, msg, style);
            return;
        }

        for (i, &(idx, _score)) in self.filtered.iter().enumerate() {
            if i as u16 >= list_area.height {
                break;
            }

            let y = list_area.y + i as u16;
            let prefix = if i == self.selected_index { "> " } else { "  " };
            let style = if i == self.selected_index {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };

            let line = format!("{}{}", prefix, self.apps[idx].name);
            buf.set_string(list_area.x, y, &line, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn make_key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

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
        }
    }

    #[test]
    fn down_key_moves_selection_down() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Down);
        launcher.handle_event(&event);
        assert_eq!(launcher.selected_index, 1);
    }

    #[test]
    fn up_key_moves_selection_up() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 1;
        let event = make_key_event(KeyCode::Up);
        launcher.handle_event(&event);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn down_wraps_to_first() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 2;
        let event = make_key_event(KeyCode::Down);
        launcher.handle_event(&event);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn up_wraps_to_last() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 0;
        let event = make_key_event(KeyCode::Up);
        launcher.handle_event(&event);
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn enter_stops_launcher() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Enter);
        launcher.handle_event(&event);
        assert!(!launcher.running);
    }

    #[test]
    fn escape_stops_launcher() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Esc);
        launcher.handle_event(&event);
        assert!(!launcher.running);
    }

    #[test]
    fn typing_filters_results() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Char('a'));
        launcher.handle_event(&event);
        assert_eq!(launcher.query, "a");
        assert!(!launcher.filtered.is_empty());
    }

    #[test]
    fn backspace_removes_char() {
        let mut launcher = make_launcher_with_apps();
        launcher.query = "a".into();
        let event = make_key_event(KeyCode::Backspace);
        launcher.handle_event(&event);
        assert_eq!(launcher.query, "");
    }

    #[test]
    fn tab_autocompletes_top_match() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Char('a'));
        launcher.handle_event(&event);
        let event = make_key_event(KeyCode::Tab);
        launcher.handle_event(&event);
        assert!(!launcher.query.is_empty());
    }

    #[test]
    fn page_down_moves_selection_down() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::PageDown);
        launcher.handle_event(&event);
        assert_eq!(launcher.selected_index, 10.min(launcher.filtered.len() - 1));
    }

    #[test]
    fn page_up_moves_selection_up() {
        let mut launcher = make_launcher_with_apps();
        launcher.selected_index = 15;
        let event = make_key_event(KeyCode::PageUp);
        launcher.handle_event(&event);
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
        };
        let event = make_key_event(KeyCode::Esc);
        launcher.handle_event(&event);
        assert!(!launcher.running);
    }

    #[test]
    fn search_rebuilds_filtered_on_typing() {
        let mut launcher = make_launcher_with_apps();
        let event = make_key_event(KeyCode::Char('A'));
        launcher.handle_event(&event);
        assert_eq!(launcher.query, "A");
        assert!(launcher.selected_index == 0);
    }

    #[test]
    fn enter_records_launch_in_history() {
        let history = History::in_memory().unwrap();
        let mut launcher = make_launcher_with_apps();
        launcher.history = Some(history);

        let event = make_key_event(KeyCode::Enter);
        launcher.handle_event(&event);

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
    fn empty_query_places_most_used_first() {
        let mut launcher = make_launcher_with_apps();
        launcher.scores.insert("app-c".to_string(), 42.0);
        launcher.rebuild_filtered();

        let (top_idx, _) = launcher.filtered[0];
        assert_eq!(launcher.apps[top_idx].id, "app-c");
    }
}
