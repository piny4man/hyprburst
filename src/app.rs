use ratatui::prelude::*;

use crate::config::Config;
use crate::effects::FadeIn;
use crate::launcher::Launcher;

pub struct App {
    pub running: bool,
    launcher: Launcher,
    fade_in: FadeIn,
}

impl App {
    pub fn new(config: Config) -> Self {
        let launcher = Launcher::new(config);
        let running = launcher.running;
        Self {
            running,
            launcher,
            fade_in: FadeIn::new(),
        }
    }

    pub fn handle_event(&mut self, event: &crossterm::event::Event) {
        self.launcher.handle_event(event);
        self.running = self.launcher.running;
    }

    pub fn apply_effects(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if !self.fade_in.is_done() {
            self.fade_in.apply(frame, area);
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.launcher.render(area, buf);
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
        let app = App::new(Config::default());
        assert!(app.running);
    }

    #[test]
    fn escape_stops_app() {
        let mut app = App::new(Config::default());
        let event = make_key_event(KeyCode::Esc);
        app.handle_event(&event);
        assert!(!app.running);
    }

    #[test]
    fn other_keys_keep_app_running() {
        let mut app = App::new(Config::default());
        for code in [
            KeyCode::Tab,
            KeyCode::Char('a'),
            KeyCode::Char('q'),
            KeyCode::Left,
            KeyCode::Right,
        ] {
            app.running = true;
            let event = make_key_event(code);
            app.handle_event(&event);
            assert!(app.running, "App should still running after {:?}", code);
        }
    }
}
