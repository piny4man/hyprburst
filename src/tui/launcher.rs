//! Thin ratatui frontend for the launcher.
//!
//! All state and behavior live in [`LauncherCore`]; this module only maps
//! crossterm `KeyCode`s to [`LauncherAction`]s and renders the core's view via the
//! shared [`render_core`](crate::view::render::render_core).

use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::prelude::*;

use crate::domain::config::Config;
use crate::domain::launcher_core::{LauncherAction, LauncherCore};
use crate::view::layout::entry_at;
use crate::view::render::{RenderCache, render_core};

pub struct Launcher {
    core: LauncherCore,
    cache: RenderCache,
    last_area: Rect,
}

impl Launcher {
    pub fn new(config: Config) -> Self {
        Self {
            core: LauncherCore::new(config),
            cache: RenderCache::new(),
            last_area: Rect::default(),
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

    pub fn handle_mouse(&mut self, event: MouseEvent) {
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }

        let entry_count = self.core.view().entries.len();
        if let Some(index) = entry_at(
            self.last_area,
            self.core.config(),
            (event.column, event.row),
            entry_count,
        ) {
            self.core.apply(LauncherAction::SelectEntry(index));
            self.core.apply(LauncherAction::LaunchSelected);
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
        self.last_area = area;
        // Fill before rendering so ordinary text and blank cells inherit the
        // base palette, while explicit styles (including Reset) still win.
        // Rio's GPU-host child supplies Reset here: its parent owns the base
        // colors and translucent panel, not opaque per-cell backgrounds.
        let colors = &self.core.config().colors;
        buf.set_style(
            area,
            Style::new().fg(colors.foreground).bg(colors.background),
        );
        render_core(&mut self.cache, &mut self.core, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themed_config(overrides: &str) -> Config {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/themes");
        let (config, warnings) = Config::from_toml_str_validating_at(
            &format!("[appearance]\ntheme_file = \"base.toml\"\n{overrides}"),
            Some(&dir),
            None,
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        config
    }

    fn launcher_with_apps(mut config: Config) -> Launcher {
        use crate::domain::{config::LayoutMode, desktop::DesktopEntry};
        config.layout.mode = LayoutMode::List;
        config.layout.padding_horizontal = 1;
        config.layout.padding_vertical = 1;
        config.layout.separator = false;
        config.ui.show_banner = false;
        config.ui.show_icons = false;
        Launcher {
            core: LauncherCore::for_test(
                vec![
                    DesktopEntry::new("alpha", "Alpha", "true", ""),
                    DesktopEntry::new("beta", "Beta", "true", ""),
                ],
                config,
            ),
            cache: RenderCache::new(),
            last_area: Rect::default(),
        }
    }

    fn draw(launcher: &mut Launcher) -> Buffer {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 10)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(launcher, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn external_tui_applies_theme_and_explicit_base_overrides() {
        for (overrides, fg, bg) in [
            (
                "",
                Color::Rgb(0xea, 0xf3, 0xff),
                Color::Rgb(0x10, 0x25, 0x3f),
            ),
            (
                "[colors]\nforeground = \"#123456\"\nbackground = \"#654321\"",
                Color::Rgb(0x12, 0x34, 0x56),
                Color::Rgb(0x65, 0x43, 0x21),
            ),
            (
                "[colors]\nforeground = \"reset\"",
                Color::Reset,
                Color::Rgb(0x10, 0x25, 0x3f),
            ),
            (
                "[colors]\nbackground = \"reset\"",
                Color::Rgb(0xea, 0xf3, 0xff),
                Color::Reset,
            ),
        ] {
            let mut launcher = launcher_with_apps(themed_config(overrides));
            let buf = draw(&mut launcher);
            assert_eq!(buf[(3, 3)].symbol(), "B");
            for pos in [(3, 3), (0, 0), (39, 9)] {
                assert_eq!((buf[pos].fg, buf[pos].bg), (fg, bg), "at {pos:?}");
            }
            assert_eq!(buf[(1, 1)].fg, launcher.core.config().colors.prompt);
            assert_eq!(buf[(1, 1)].bg, bg);
        }
    }

    #[test]
    fn selected_colors_and_explicit_reset_survive_base_fill() {
        for overrides in [
            "",
            "[colors]\nselected = \"reset\"\nselected_bg = \"reset\"",
        ] {
            let mut launcher = launcher_with_apps(themed_config(overrides));
            let buf = draw(&mut launcher);
            let colors = &launcher.core.config().colors;
            for x in 1..39 {
                assert_eq!(
                    (buf[(x, 2)].fg, buf[(x, 2)].bg),
                    (colors.selected, colors.selected_bg)
                );
                assert!(buf[(x, 2)].modifier.contains(Modifier::BOLD));
            }
            assert_eq!(buf[(3, 3)].fg, colors.foreground);
        }
    }

    #[test]
    fn no_theme_uses_builtin_palette_and_reset_opts_into_terminal_defaults() {
        for source in [
            "",
            "[colors]\nforeground = \"reset\"\nbackground = \"reset\"",
        ] {
            let mut launcher = launcher_with_apps(Config::from_toml_str(source).unwrap());
            let buf = draw(&mut launcher);
            let colors = &launcher.core.config().colors;
            for pos in [(3, 3), (39, 9)] {
                assert_eq!(
                    (buf[pos].fg, buf[pos].bg),
                    (colors.foreground, colors.background)
                );
            }
        }
    }

    #[test]
    fn base_fill_is_limited_to_widget_area_and_covers_empty_results() {
        let mut launcher = launcher_with_apps(themed_config(""));
        launcher.handle_key(KeyCode::Char('z'));
        let area = Rect::new(2, 2, 36, 6);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 10));
        (&mut launcher).render(area, &mut buf);
        assert_eq!(
            (buf[(0, 0)].fg, buf[(0, 0)].bg),
            (Color::Reset, Color::Reset)
        );
        assert_eq!(buf[(3, 4)].symbol(), "N");
        assert_eq!(buf[(3, 4)].fg, launcher.core.config().colors.empty);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                assert_eq!(buf[(x, y)].bg, launcher.core.config().colors.background);
            }
        }
    }

    #[test]
    fn shared_gpu_buffer_keeps_default_backgrounds_unpainted() {
        let mut launcher = launcher_with_apps(themed_config(""));
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        render_core(&mut launcher.cache, &mut launcher.core, area, &mut buf);
        for pos in [(3, 3), (0, 0), (39, 9)] {
            assert_eq!((buf[pos].fg, buf[pos].bg), (Color::Reset, Color::Reset));
        }
        assert_eq!(buf[(1, 2)].bg, launcher.core.config().colors.selected_bg);
    }

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
