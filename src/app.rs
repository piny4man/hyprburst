use ratatui::prelude::*;

pub struct App {
    pub running: bool,
}

impl App {
    pub fn new() -> Self {
        Self { running: true }
    }

    pub fn handle_event(&mut self, event: &crossterm::event::Event) {
        if crate::input::is_escape(event) {
            self.running = false;
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let banner = Line::from(vec![
            Span::styled(
                "Burst",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" - Press Escape to exit"),
        ]);
        banner.render(area, buf);
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

    #[test]
    fn app_starts_running() {
        let app = App::new();
        assert!(app.running);
    }

    #[test]
    fn escape_stops_app() {
        let mut app = App::new();
        let event = make_key_event(KeyCode::Esc);
        app.handle_event(&event);
        assert!(!app.running);
    }

    #[test]
    fn other_keys_keep_app_running() {
        let mut app = App::new();
        for code in [
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Char('a'),
            KeyCode::Char('q'),
            KeyCode::Up,
            KeyCode::Down,
        ] {
            app.running = true;
            let event = make_key_event(code);
            app.handle_event(&event);
            assert!(app.running, "App should still running after {:?}", code);
        }
    }
}
