//! Thin ratatui frontend for the launcher.
//!
//! All state and behavior live in [`LauncherCore`]; this module only maps
//! crossterm `KeyCode`s to [`LauncherAction`]s and renders the core's view via the
//! shared [`render_core`](crate::view::render::render_core).

use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;

use crate::domain::config::Config;
use crate::domain::launcher_core::{LauncherAction, LauncherCore};
use crate::view::render::render_core;

pub struct Launcher {
    core: LauncherCore,
}

impl Launcher {
    pub fn new(config: Config) -> Self {
        Self {
            core: LauncherCore::new(config),
        }
    }

    pub fn running(&self) -> bool {
        self.core.running()
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if let Some(action) = key_to_action(code) {
            self.core.apply(action);
        }
    }
}

/// Translate a crossterm key into an abstract [`LauncherAction`]. Keys with no
/// launcher meaning return `None`.
fn key_to_action(code: KeyCode) -> Option<LauncherAction> {
    Some(match code {
        KeyCode::Esc => LauncherAction::Cancel,
        KeyCode::Tab => LauncherAction::Autocomplete,
        KeyCode::PageUp => LauncherAction::PageUp,
        KeyCode::PageDown => LauncherAction::PageDown,
        KeyCode::Up => LauncherAction::MoveUp,
        KeyCode::Down => LauncherAction::MoveDown,
        KeyCode::Left => LauncherAction::MoveLeft,
        KeyCode::Right => LauncherAction::MoveRight,
        KeyCode::Enter => LauncherAction::LaunchSelected,
        KeyCode::Backspace => LauncherAction::Backspace,
        KeyCode::Char(c) => LauncherAction::Insert(c),
        _ => return None,
    })
}

impl Widget for &mut Launcher {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_core(&mut self.core, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_to_action_maps_navigation_and_text_keys() {
        assert_eq!(key_to_action(KeyCode::Esc), Some(LauncherAction::Cancel));
        assert_eq!(
            key_to_action(KeyCode::Enter),
            Some(LauncherAction::LaunchSelected)
        );
        assert_eq!(
            key_to_action(KeyCode::Tab),
            Some(LauncherAction::Autocomplete)
        );
        assert_eq!(key_to_action(KeyCode::Up), Some(LauncherAction::MoveUp));
        assert_eq!(key_to_action(KeyCode::Down), Some(LauncherAction::MoveDown));
        assert_eq!(key_to_action(KeyCode::Left), Some(LauncherAction::MoveLeft));
        assert_eq!(
            key_to_action(KeyCode::Right),
            Some(LauncherAction::MoveRight)
        );
        assert_eq!(key_to_action(KeyCode::PageUp), Some(LauncherAction::PageUp));
        assert_eq!(
            key_to_action(KeyCode::PageDown),
            Some(LauncherAction::PageDown)
        );
        assert_eq!(
            key_to_action(KeyCode::Backspace),
            Some(LauncherAction::Backspace)
        );
        assert_eq!(
            key_to_action(KeyCode::Char('x')),
            Some(LauncherAction::Insert('x'))
        );
        assert_eq!(key_to_action(KeyCode::Home), None);
    }
}
