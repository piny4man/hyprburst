use std::process::Command;

use ratatui::prelude::*;

use crate::desktop::{DesktopEntry, discover_apps};

pub struct Launcher {
    pub apps: Vec<DesktopEntry>,
    pub selected_index: usize,
    pub running: bool,
}

impl Launcher {
    pub fn new() -> Self {
        let apps = discover_apps();
        let running = !apps.is_empty();
        Self {
            apps,
            selected_index: 0,
            running,
        }
    }

    pub fn handle_event(&mut self, event: &crossterm::event::Event) {
        if self.apps.is_empty() {
            if crate::input::is_escape(event) {
                self.running = false;
            }
            return;
        }

        if crate::input::is_escape(event) {
            self.running = false;
        } else if crate::input::is_up(event) {
            if self.selected_index == 0 {
                self.selected_index = self.apps.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        } else if crate::input::is_down(event) {
            if self.selected_index == self.apps.len() - 1 {
                self.selected_index = 0;
            } else {
                self.selected_index += 1;
            }
        } else if crate::input::is_enter(event) {
            self.launch_selected();
        }
    }

    fn launch_selected(&mut self) {
        if self.selected_index < self.apps.len() {
            let exec = &self.apps[self.selected_index].exec;
            if !exec.is_empty() {
                let _ = Command::new("hyprctl")
                    .args(["dispatch", "exec", "--", exec])
                    .spawn();
            }
        }
        self.running = false;
    }
}

impl Widget for &mut Launcher {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.apps.is_empty() {
            let msg = "No applications found";
            let style = Style::new().fg(Color::Yellow);
            buf.set_string(area.x, area.y, msg, style);
            return;
        }

        let title = Line::from(vec![
            Span::styled(
                "Burst",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" - Arrow keys to navigate, Enter to launch, Esc to exit"),
        ]);
        title.render(area, buf);

        let list_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };

        for (i, app) in self.apps.iter().enumerate() {
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

            let line = format!("{}{}", prefix, app.name);
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
                    name: "App A".into(),
                    icon: "icon-a".into(),
                    exec: "app-a".into(),
                },
                DesktopEntry {
                    name: "App B".into(),
                    icon: "icon-b".into(),
                    exec: "app-b".into(),
                },
                DesktopEntry {
                    name: "App C".into(),
                    icon: "icon-c".into(),
                    exec: "app-c".into(),
                },
            ],
            selected_index: 0,
            running: true,
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
    fn empty_apps_list_stops_on_escape() {
        let mut launcher = Launcher {
            apps: vec![],
            selected_index: 0,
            running: true,
        };
        let event = make_key_event(KeyCode::Esc);
        launcher.handle_event(&event);
        assert!(!launcher.running);
    }

    #[test]
    fn empty_apps_list_ignores_navigation() {
        let mut launcher = Launcher {
            apps: vec![],
            selected_index: 0,
            running: true,
        };
        let event = make_key_event(KeyCode::Down);
        launcher.handle_event(&event);
        assert!(launcher.running);
    }
}
