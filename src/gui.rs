//! Freya native-GUI frontend for the launcher (feature `freya-spike`).
//!
//! Like the ratatui [`launcher`](crate::launcher) frontend, this is a thin
//! renderer over the shared [`LauncherCore`]: it maps Freya keyboard events to
//! abstract [`LauncherAction`]s and renders the core's
//! [`LauncherView`](crate::launcher_core::LauncherView) projection. No
//! selection, filtering, or launch logic lives here — the core owns all of it.
//!
//! Rendering is split in two so the benchmark harness can measure per-frame CPU
//! work without a GPU surface or the Dioxus runtime:
//!
//! - [`build_frame`] is pure: it turns a [`LauncherView`] into a frontend-
//!   agnostic [`GuiFrame`] (banner lines, prompt string, grid cells). This is
//!   the analog of filling a ratatui [`Buffer`](ratatui::buffer::Buffer) — the
//!   work done every frame *before* anything is composited — so the harness
//!   paints by calling it, exactly as the baseline column paints via
//!   `render_core`.
//! - [`render_frame`] maps a [`GuiFrame`] into the actual Freya element tree
//!   (`rect`/text nodes with colors). It is only ever called inside the live
//!   window's component, where the Freya runtime is present.

use freya::prelude::*;
use ratatui::style::Color;

use crate::config::Config;
use crate::launcher_core::{EmptyReason, LauncherAction, LauncherView};

/// Number of grid columns the POC window renders. A fixed value (rather than the
/// config's list/grid mode) so grid navigation is always exercised end-to-end on
/// the 640 px probe window.
pub const GRID_COLUMNS: u16 = 3;

/// Window background; matches the `WindowConfig` background so transparency/blur
/// windowrules blend cleanly.
pub const BG: (u8, u8, u8) = (20, 20, 28);
/// Background fill behind the selected cell.
pub const SELECTED_BG: (u8, u8, u8) = (40, 42, 60);
/// Default foreground text color for unselected entries.
pub const FG: (u8, u8, u8) = (220, 220, 230);

/// One rendered grid cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiCell {
    /// The text to draw (`"<glyph> <name>"`, or just the name when icons are off).
    pub text: String,
    pub selected: bool,
}

/// Frontend-agnostic snapshot of what a single frame should draw. Produced from
/// a [`LauncherView`] by [`build_frame`]; consumed by [`render_frame`] (live) or
/// measured directly (harness).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiFrame {
    pub banner_lines: Vec<String>,
    pub prompt: String,
    /// `Some(msg)` when there are no entries to show; `rows` is then empty.
    pub empty_message: Option<&'static str>,
    /// Entries chunked into grid rows of up to `view.columns` cells.
    pub rows: Vec<Vec<GuiCell>>,
}

/// Translate a Freya keyboard key into an abstract [`LauncherAction`]. Keys with
/// no launcher meaning (and multi-character composed input) return `None`. This
/// is the GUI analog of the TUI's `key_to_action`, sharing the exact same action
/// vocabulary so both frontends drive the core identically.
pub fn key_to_action(key: &Key) -> Option<LauncherAction> {
    match key {
        Key::Named(named) => Some(match named {
            NamedKey::Escape => LauncherAction::Cancel,
            NamedKey::Tab => LauncherAction::Autocomplete,
            NamedKey::PageUp => LauncherAction::PageUp,
            NamedKey::PageDown => LauncherAction::PageDown,
            NamedKey::ArrowUp => LauncherAction::MoveUp,
            NamedKey::ArrowDown => LauncherAction::MoveDown,
            NamedKey::ArrowLeft => LauncherAction::MoveLeft,
            NamedKey::ArrowRight => LauncherAction::MoveRight,
            NamedKey::Enter => LauncherAction::LaunchSelected,
            NamedKey::Backspace => LauncherAction::Backspace,
            _ => return None,
        }),
        Key::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            // Ignore composed/multi-codepoint input — the launcher inserts one
            // character at a time, matching the TUI's `KeyCode::Char(c)`.
            if chars.next().is_some() {
                return None;
            }
            Some(LauncherAction::Insert(c))
        }
    }
}

/// Build the per-frame render model from the core's view. Pure CPU work, no
/// Freya runtime required — this is what the harness times each frame.
pub fn build_frame(view: &LauncherView, config: &Config) -> GuiFrame {
    let banner_lines = if config.ui.banner.is_empty() {
        Vec::new()
    } else {
        config.ui.banner.lines().map(str::to_string).collect()
    };

    let prompt = format!("{}{}", config.ui.prompt, view.query);

    if view.entries.is_empty() {
        let msg = match view.empty_reason {
            Some(EmptyReason::NoMatches) => "No matches",
            _ => "No applications found",
        };
        return GuiFrame {
            banner_lines,
            prompt,
            empty_message: Some(msg),
            rows: Vec::new(),
        };
    }

    let cols = view.columns.max(1) as usize;
    let rows = view
        .entries
        .chunks(cols)
        .map(|chunk| {
            chunk
                .iter()
                .map(|entry| {
                    let text = if config.ui.show_icons {
                        format!("{} {}", entry.icon_glyph, entry.name)
                    } else {
                        entry.name.to_string()
                    };
                    GuiCell {
                        text,
                        selected: entry.selected,
                    }
                })
                .collect()
        })
        .collect();

    GuiFrame {
        banner_lines,
        prompt,
        empty_message: None,
        rows,
    }
}

/// Map a [`GuiFrame`] into a Freya element tree. Only called inside the live
/// window component (the Freya runtime must be active).
pub fn render_frame(frame: &GuiFrame, config: &Config) -> Element {
    let banner_rgb = color_rgb(config.colors.banner);
    let prompt_rgb = color_rgb(config.colors.prompt);
    let selected_rgb = color_rgb(config.colors.selected);
    let empty_rgb = color_rgb(config.colors.empty);

    let mut children: Vec<Element> = Vec::new();

    // Banner block — one text node per line (the ASCII banner is multi-line).
    if !frame.banner_lines.is_empty() {
        let lines: Vec<Element> = frame
            .banner_lines
            .iter()
            .map(|line| text_node(line.clone(), banner_rgb, 14.0))
            .collect();
        children.push(
            rect()
                .direction(Direction::vertical())
                .children(lines)
                .into_element(),
        );
    }

    // Search prompt.
    children.push(text_node(frame.prompt.clone(), prompt_rgb, 22.0));

    // List/grid, or the empty-state message.
    if let Some(msg) = frame.empty_message {
        children.push(text_node(msg.to_string(), empty_rgb, 18.0));
    } else {
        let rows: Vec<Element> = frame
            .rows
            .iter()
            .map(|row| {
                let cells: Vec<Element> = row
                    .iter()
                    .map(|cell| {
                        let (fg, bg) = if cell.selected {
                            (selected_rgb, SELECTED_BG)
                        } else {
                            (FG, BG)
                        };
                        rect()
                            .padding(Gaps::new_all(6.0))
                            .background(bg)
                            .color(fg)
                            .font_size(18.0)
                            .child(cell.text.clone())
                            .into_element()
                    })
                    .collect();
                rect()
                    .direction(Direction::horizontal())
                    .spacing(12.0)
                    .children(cells)
                    .into_element()
            })
            .collect();
        children.push(
            rect()
                .direction(Direction::vertical())
                .spacing(6.0)
                .children(rows)
                .into_element(),
        );
    }

    rect()
        .expanded()
        .direction(Direction::vertical())
        .padding(Gaps::new_all(20.0))
        .spacing(16.0)
        .background(BG)
        .color(FG)
        .children(children)
        .into_element()
}

/// A single text node with an explicit color and font size.
fn text_node(text: String, color: (u8, u8, u8), font_size: f32) -> Element {
    rect()
        .color(color)
        .font_size(font_size)
        .child(text)
        .into_element()
}

/// Map a ratatui [`Color`] (the launcher's config color type) to an RGB triple
/// Freya can paint. Named ANSI colors use conventional terminal RGB values so
/// the GUI's accent colors track the TUI config.
pub fn color_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (118, 118, 118),
        Color::DarkGray => (102, 102, 102),
        Color::LightRed => (241, 76, 76),
        Color::LightGreen => (35, 209, 139),
        Color::LightYellow => (245, 245, 67),
        Color::LightBlue => (59, 142, 234),
        Color::LightMagenta => (214, 112, 214),
        Color::LightCyan => (41, 184, 219),
        Color::White | Color::Reset => (229, 229, 229),
        Color::Indexed(_) => FG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::DesktopEntry;
    use crate::launcher_core::LauncherCore;

    fn apps(n: usize) -> Vec<DesktopEntry> {
        (0..n)
            .map(|i| DesktopEntry {
                id: format!("app-{i}"),
                name: format!("App {i}"),
                icon: format!("icon-{i}"),
                exec: format!("app-{i}"),
            })
            .collect()
    }

    fn ch(c: char) -> Key {
        Key::Character(c.to_string())
    }

    #[test]
    fn key_to_action_maps_named_navigation_keys() {
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::Escape)),
            Some(LauncherAction::Cancel)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::Enter)),
            Some(LauncherAction::LaunchSelected)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::Tab)),
            Some(LauncherAction::Autocomplete)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::ArrowUp)),
            Some(LauncherAction::MoveUp)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::ArrowDown)),
            Some(LauncherAction::MoveDown)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::ArrowLeft)),
            Some(LauncherAction::MoveLeft)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::ArrowRight)),
            Some(LauncherAction::MoveRight)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::PageUp)),
            Some(LauncherAction::PageUp)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::PageDown)),
            Some(LauncherAction::PageDown)
        );
        assert_eq!(
            key_to_action(&Key::Named(NamedKey::Backspace)),
            Some(LauncherAction::Backspace)
        );
    }

    #[test]
    fn key_to_action_maps_single_character_to_insert() {
        assert_eq!(key_to_action(&ch('a')), Some(LauncherAction::Insert('a')));
        assert_eq!(key_to_action(&ch('Z')), Some(LauncherAction::Insert('Z')));
    }

    #[test]
    fn key_to_action_ignores_multi_char_and_unmapped_keys() {
        assert_eq!(key_to_action(&Key::Character("ab".to_string())), None);
        assert_eq!(key_to_action(&Key::Character(String::new())), None);
        assert_eq!(key_to_action(&Key::Named(NamedKey::F1)), None);
    }

    #[test]
    fn build_frame_chunks_entries_into_grid_rows() {
        let mut core = LauncherCore::for_test(apps(7), Config::default());
        core.set_columns(3);
        let view = core.view();
        let frame = build_frame(&view, core.config());

        // 7 entries, 3 columns → rows of 3, 3, 1.
        let row_lens: Vec<usize> = frame.rows.iter().map(Vec::len).collect();
        assert_eq!(row_lens, vec![3, 3, 1]);
    }

    #[test]
    fn build_frame_marks_the_selected_cell() {
        let mut core = LauncherCore::for_test(apps(4), Config::default());
        core.set_columns(2);
        core.apply(LauncherAction::MoveDown); // select index 2 (row 1, col 0)
        let view = core.view();
        let frame = build_frame(&view, core.config());

        let selected: Vec<(usize, usize)> = frame
            .rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| {
                row.iter()
                    .enumerate()
                    .filter(|(_, c)| c.selected)
                    .map(move |(col, _)| (r, col))
            })
            .collect();
        assert_eq!(selected, vec![(1, 0)]);
    }

    #[test]
    fn build_frame_formats_icon_and_name() {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        let core = LauncherCore::for_test(vec![app], Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config());

        let cell = &frame.rows[0][0];
        assert!(
            cell.text.contains('\u{f269}') && cell.text.contains("Firefox"),
            "expected glyph + name, got {:?}",
            cell.text
        );
    }

    #[test]
    fn build_frame_hides_icon_when_disabled() {
        let cfg = Config {
            ui: crate::config::UiConfig {
                show_icons: false,
                ..crate::config::UiConfig::default()
            },
            ..Config::default()
        };
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        let core = LauncherCore::for_test(vec![app], cfg);
        let view = core.view();
        let frame = build_frame(&view, core.config());

        assert_eq!(frame.rows[0][0].text, "Firefox");
    }

    #[test]
    fn build_frame_includes_prompt_with_query() {
        let mut core = LauncherCore::for_test(apps(3), Config::default());
        core.apply(LauncherAction::Insert('a'));
        let view = core.view();
        let frame = build_frame(&view, core.config());
        assert_eq!(frame.prompt, "> a");
    }

    #[test]
    fn build_frame_reports_empty_state_message() {
        let core = LauncherCore::for_test(vec![], Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config());
        assert!(frame.rows.is_empty());
        assert_eq!(frame.empty_message, Some("No applications found"));
    }

    #[test]
    fn build_frame_splits_banner_into_lines() {
        let cfg = Config {
            ui: crate::config::UiConfig {
                banner: "line one\nline two".into(),
                ..crate::config::UiConfig::default()
            },
            ..Config::default()
        };
        let core = LauncherCore::for_test(apps(1), cfg);
        let view = core.view();
        let frame = build_frame(&view, core.config());
        assert_eq!(frame.banner_lines, vec!["line one", "line two"]);
    }
}
