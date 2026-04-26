use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;

use crate::config::Config;
use crate::desktop::{DesktopEntry, discover_apps};
use crate::effects::FadeIn;
use crate::history::History;
use crate::launcher::load_scores;
use crate::startup::StartupState;

const FAST_DISCOVERY_GRACE: Duration = Duration::from_millis(16);

pub type DiscoveryReceiver = Receiver<Result<DiscoveryPayload, String>>;

pub struct DiscoveryPayload {
    pub apps: Vec<DesktopEntry>,
    pub scores: HashMap<String, f64>,
}

impl DiscoveryPayload {
    pub fn from_apps(apps: Vec<DesktopEntry>) -> Self {
        Self {
            apps,
            scores: HashMap::new(),
        }
    }
}

pub struct App {
    pub running: bool,
    startup: StartupState,
    fade_in: FadeIn,
    discovery: Option<DiscoveryReceiver>,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self::with_discovery_receiver(config, spawn_discovery(), FAST_DISCOVERY_GRACE)
    }

    pub fn with_discovery_receiver(
        config: Config,
        discovery: DiscoveryReceiver,
        fast_grace: Duration,
    ) -> Self {
        let (startup, discovery) = match discovery.recv_timeout(fast_grace) {
            Ok(Ok(payload)) => (startup_from_payload(config, payload, String::new()), None),
            Ok(Err(error)) => (StartupState::failed(config, String::new(), error), None),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                (StartupState::loading(config), Some(discovery))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => (
                StartupState::failed(
                    config,
                    String::new(),
                    "discovery worker stopped before returning results".to_string(),
                ),
                None,
            ),
        };
        let running = startup.is_running();
        Self {
            running,
            startup,
            fade_in: FadeIn::new(),
            discovery,
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        self.startup.handle_key(code);
        self.running = self.startup.is_running();
        self.poll_discovery();
    }

    pub fn poll_discovery(&mut self) {
        if !self.running {
            return;
        }

        let Some(discovery) = self.discovery.take() else {
            return;
        };

        match discovery.try_recv() {
            Ok(Ok(payload)) => {
                self.startup.finish_loading(payload.apps, payload.scores);
                self.running = self.startup.is_running();
            }
            Ok(Err(error)) => {
                self.startup.fail_loading(error);
                self.running = self.startup.is_running();
            }
            Err(TryRecvError::Empty) => {
                self.discovery = Some(discovery);
            }
            Err(TryRecvError::Disconnected) => {
                self.startup
                    .fail_loading("discovery worker stopped before returning results".to_string());
                self.running = self.startup.is_running();
            }
        }
    }

    pub fn apply_effects(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if !self.fade_in.is_done() {
            self.fade_in.apply(frame, area);
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.poll_discovery();
        self.startup.render(area, buf);
    }
}

fn spawn_discovery() -> DiscoveryReceiver {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let apps = discover_apps();
        let scores = History::open()
            .ok()
            .as_ref()
            .map(load_scores)
            .unwrap_or_default();
        let _ = tx.send(Ok(DiscoveryPayload { apps, scores }));
    });
    rx
}

fn startup_from_payload(config: Config, payload: DiscoveryPayload, query: String) -> StartupState {
    let mut startup = StartupState::loading(config);
    startup.finish_loading(payload.apps, payload.scores);
    if !query.is_empty() {
        for c in query.chars() {
            startup.handle_key(KeyCode::Char(c));
        }
    }
    startup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_starts_running() {
        let (_tx, rx) = mpsc::channel();
        let app = App::with_discovery_receiver(Config::default(), rx, Duration::ZERO);
        assert!(app.running);
    }

    #[test]
    fn escape_stops_app() {
        let (_tx, rx) = mpsc::channel();
        let mut app = App::with_discovery_receiver(Config::default(), rx, Duration::ZERO);
        app.handle_key(KeyCode::Esc);
        assert!(!app.running);
    }

    #[test]
    fn other_keys_keep_app_running() {
        let (_tx, rx) = mpsc::channel();
        let mut app = App::with_discovery_receiver(Config::default(), rx, Duration::ZERO);
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
