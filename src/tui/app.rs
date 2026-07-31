use ratatui::crossterm::event::{KeyCode, MouseEvent};
use ratatui::prelude::*;

use crate::domain::config::Config;
use crate::tui::effects::FadeIn;
use crate::tui::launcher::Launcher;

pub struct App {
    pub running: bool,
    launcher: Launcher,
    fade_in: FadeIn,
}

impl App {
    pub fn new(config: Config) -> Self {
        let launcher = Launcher::new(config);
        let running = launcher.running();
        Self {
            running,
            launcher,
            fade_in: FadeIn::new(),
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        self.launcher.handle_key(code);
        self.running = self.launcher.running();
    }

    pub fn handle_mouse(&mut self, event: MouseEvent) {
        self.launcher.handle_mouse(event);
        self.running = self.launcher.running();
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

    #[test]
    fn app_starts_running() {
        let app = App::new(Config::default());
        assert!(app.running);
    }

    #[test]
    fn escape_stops_app() {
        let mut app = App::new(Config::default());
        app.handle_key(KeyCode::Esc);
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
            app.handle_key(code);
            assert!(app.running, "App should still running after {:?}", code);
        }
    }
}
