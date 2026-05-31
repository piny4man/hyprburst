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

use std::path::PathBuf;

use freya::prelude::*;
use ratatui::style::Color;

use crate::config::{Config, LayoutMode};
use crate::launcher_core::{EmptyReason, LauncherAction, LauncherView};

/// Probe-window geometry, shared with the `hyprburst-spike-gui` `WindowConfig`
/// so the column math and the actual surface agree.
pub const WINDOW_WIDTH: f32 = 640.0;
pub const WINDOW_HEIGHT: f32 = 720.0;
/// Uniform padding around the root content, in px.
pub const ROOT_PADDING: f32 = 20.0;

/// Font sizes for the three rendered regions (banner / prompt / entry rows).
pub const BANNER_FONT_SIZE: f32 = 14.0;
pub const PROMPT_FONT_SIZE: f32 = 22.0;
pub const ENTRY_FONT_SIZE: f32 = 18.0;

/// Generic monospace family, used when no explicit override is set. fontconfig
/// resolves it to the system's configured monospace face (a Nerd Font on most
/// Hyprland setups), so the banner ASCII art and grid columns align cell-for-cell
/// like the terminal — the single biggest visual-parity fix over Freya's
/// proportional default face. See [`font_family`] for the override.
pub const MONOSPACE_FAMILY: &str = "monospace";

/// Environment variable that pins the GUI font family to an exact face (e.g. the
/// user's terminal Nerd Font), for closer parity than the generic `monospace`
/// alias can guarantee.
pub const FONT_ENV: &str = "HYPRBURST_GUI_FONT";

/// Approximate advance width of one monospace glyph as a fraction of its font
/// size; used only to estimate how many grid columns fit the window.
const MONO_CHAR_WIDTH_RATIO: f32 = 0.6;

/// Translucent panel fill painted behind everything. The alpha (< 1) is what
/// lets a Hyprland `blur` windowrule for app-id `hyprburst` show through — an
/// opaque fill would defeat `with_transparency(true)` and leave no pixels to
/// blur. Tuned to stay readable while clearly translucent.
pub const SURFACE: (u8, u8, u8, f32) = (20, 20, 28, 0.82);
/// Fully transparent window clear and event-surface fill, so the only
/// translucency in play is [`SURFACE`]'s — a translucent clear *and* a
/// translucent panel would stack and wash out the blur.
pub const WINDOW_CLEAR: (u8, u8, u8, u8) = (0, 0, 0, 0);
/// Default foreground text color for unselected entries.
pub const FG: (u8, u8, u8) = (220, 220, 230);

/// Square side, in px, of a themed icon rendered next to an entry name. Sized to
/// sit beside the [`ENTRY_FONT_SIZE`] text without dominating the row.
pub const ENTRY_ICON_PX: f32 = 22.0;

/// Environment variable selecting how entries draw their icon: `glyph` (default)
/// renders the Nerd Font glyph as text; `themed` resolves a real icon file from
/// the system theme and renders the image, falling back to the glyph when none
/// is found. See [`icon_mode`] and the Phase 6 bake-off notes.
pub const ICON_ENV: &str = "HYPRBURST_GUI_ICONS";

/// How the GUI draws an entry's icon. The default ([`Glyph`](Self::Glyph)) keeps
/// the Phase 4 Nerd Font path; [`Themed`](Self::Themed) is the Phase 6 measured
/// bonus that renders real themed image icons, glyph-falling-back when a theme
/// icon can't be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconMode {
    /// Render the Nerd Font glyph as text (Phase 4 path; no image decode).
    Glyph,
    /// Render a real themed icon image, falling back to the glyph when the
    /// theme has no icon for the entry.
    Themed,
}

/// Resolve the [`IconMode`] from the [`ICON_ENV`] environment variable, defaulting
/// to [`IconMode::Glyph`].
pub fn icon_mode() -> IconMode {
    resolve_icon_mode(std::env::var(ICON_ENV).ok().as_deref())
}

/// Pure core of [`icon_mode`], split out so the toggle is testable without
/// touching the process environment. Only `themed` (case-insensitive) selects the
/// image path; everything else — unset, blank, or unrecognized — stays on glyphs.
fn resolve_icon_mode(value: Option<&str>) -> IconMode {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("themed") | Some("images") | Some("image") => IconMode::Themed,
        _ => IconMode::Glyph,
    }
}

/// Resolve the font family for the GUI: the [`FONT_ENV`] override when set to a
/// non-blank value, otherwise the generic [`MONOSPACE_FAMILY`] alias.
pub fn font_family() -> std::borrow::Cow<'static, str> {
    resolve_font_family(std::env::var(FONT_ENV).ok().as_deref())
}

/// Pure core of [`font_family`], split out so the override logic is testable
/// without touching the process environment.
fn resolve_font_family(override_family: Option<&str>) -> std::borrow::Cow<'static, str> {
    match override_family {
        Some(f) if !f.trim().is_empty() => std::borrow::Cow::Owned(f.to_string()),
        _ => std::borrow::Cow::Borrowed(MONOSPACE_FAMILY),
    }
}

/// Column count for the GUI window, mirroring the TUI [`layout`](crate::layout)
/// rule: list mode is always a single column; grid mode divides the window's
/// character-equivalent content width by the configured minimum column width.
/// This is what makes the default (list) window render one app per row, exactly
/// like the shipped TUI, instead of a forced multi-column grid.
pub fn columns_for(config: &Config) -> u16 {
    match config.layout.mode {
        LayoutMode::List => 1,
        LayoutMode::Grid => {
            let content_px = WINDOW_WIDTH - 2.0 * ROOT_PADDING;
            let char_px = ENTRY_FONT_SIZE * MONO_CHAR_WIDTH_RATIO;
            let char_cols = (content_px / char_px) as u16;
            (char_cols / config.layout.min_column_width.max(1)).max(1)
        }
    }
}

/// One rendered grid cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiCell {
    /// The text to draw. In glyph mode this is `"<glyph> <name>"` (or just the
    /// name when icons are off); in themed mode with a resolved icon the glyph is
    /// dropped and the image carried in [`icon_path`](Self::icon_path) is drawn
    /// instead, so this is `"<name>"` with the selection marker prefix.
    pub text: String,
    /// Resolved themed-icon file to draw before the text. `Some` only in
    /// [`IconMode::Themed`] when the system theme has an icon for the entry;
    /// `None` otherwise, in which case the glyph in [`text`](Self::text) renders.
    pub icon_path: Option<PathBuf>,
    pub selected: bool,
}

/// Frontend-agnostic snapshot of what a single frame should draw. Produced from
/// a [`LauncherView`] by [`build_frame`]; consumed by [`render_frame`] (live) or
/// measured directly (harness).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiFrame {
    pub banner_lines: Vec<String>,
    pub prompt: String,
    /// Cursor glyph drawn immediately after the prompt, or `None` when
    /// `ui.show_cursor` is off — mirrors the TUI's cursor cell.
    pub cursor: Option<String>,
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
///
/// `mode` selects the icon path: [`IconMode::Glyph`] keeps cells text-only (the
/// harness's glyph column and the default live render); [`IconMode::Themed`]
/// resolves a real themed-icon file per entry via [`crate::icon::resolve_icon`]
/// and, on a hit, drops the glyph from the text and records the path in
/// [`GuiCell::icon_path`] for [`render_frame`] to draw — falling back to the
/// glyph text when the theme has no icon, so the launcher always renders.
pub fn build_frame(view: &LauncherView, config: &Config, mode: IconMode) -> GuiFrame {
    let banner_lines = if config.ui.banner.is_empty() {
        Vec::new()
    } else {
        config.ui.banner.lines().map(str::to_string).collect()
    };

    let prompt = format!("{}{}", config.ui.prompt, view.query);
    let cursor = config.ui.show_cursor.then(|| config.ui.cursor_char.clone());

    if view.entries.is_empty() {
        let msg = match view.empty_reason {
            Some(EmptyReason::NoMatches) => "No matches",
            _ => "No applications found",
        };
        return GuiFrame {
            banner_lines,
            prompt,
            cursor,
            empty_message: Some(msg),
            rows: Vec::new(),
        };
    }

    // Prefix every entry with the selected marker (selected) or a same-width run
    // of spaces (others), exactly like the TUI, so the marker column lines up and
    // the selected row reads identically across both frontends.
    let marker = config.ui.selected_marker.as_str();
    let unselected_prefix = " ".repeat(marker.chars().count());

    let cols = view.columns.max(1) as usize;
    let rows = view
        .entries
        .chunks(cols)
        .map(|chunk| {
            chunk
                .iter()
                .map(|entry| {
                    // In themed mode, try to resolve a real icon file; on a hit
                    // the image replaces the glyph, otherwise we keep the glyph as
                    // a fallback so the row never goes iconless.
                    let icon_path = match mode {
                        IconMode::Themed if config.ui.show_icons => {
                            crate::icon::resolve_icon(entry.icon_name)
                        }
                        _ => None,
                    };
                    let body = match (&icon_path, config.ui.show_icons) {
                        // Image will be drawn separately — text is just the name.
                        (Some(_), _) => entry.name.to_string(),
                        (None, true) => format!("{} {}", entry.icon_glyph, entry.name),
                        (None, false) => entry.name.to_string(),
                    };
                    let prefix: &str = if entry.selected {
                        marker
                    } else {
                        &unselected_prefix
                    };
                    GuiCell {
                        text: format!("{prefix}{body}"),
                        icon_path,
                        selected: entry.selected,
                    }
                })
                .collect()
        })
        .collect();

    GuiFrame {
        banner_lines,
        prompt,
        cursor,
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

    // Banner — a single tight-line-height label so the multi-line ASCII art
    // stacks cell-for-cell like terminal rows (default paragraph leading would
    // pull the box-drawing characters apart).
    if !frame.banner_lines.is_empty() {
        children.push(
            label()
                .text(frame.banner_lines.join("\n"))
                .line_height(1.0)
                .color(banner_rgb)
                .font_size(BANNER_FONT_SIZE)
                .font_weight(FontWeight::BOLD)
                .into_element(),
        );
    }

    // Search prompt with the cursor glyph drawn right after it, both in the
    // prompt color — the same composition the TUI paints.
    let prompt_text = match &frame.cursor {
        Some(c) => format!("{}{}", frame.prompt, c),
        None => frame.prompt.clone(),
    };
    children.push(
        label()
            .text(prompt_text)
            .color(prompt_rgb)
            .font_size(PROMPT_FONT_SIZE)
            .font_weight(FontWeight::BOLD)
            .into_element(),
    );

    // List/grid, or the empty-state message.
    if let Some(msg) = frame.empty_message {
        children.push(
            label()
                .text(msg.to_string())
                .color(empty_rgb)
                .font_size(ENTRY_FONT_SIZE)
                .into_element(),
        );
    } else {
        let rows: Vec<Element> = frame
            .rows
            .iter()
            .map(|row| {
                let cells: Vec<Element> = row
                    .iter()
                    .map(|cell| render_cell(cell, selected_rgb))
                    .collect();
                rect()
                    .direction(Direction::horizontal())
                    .spacing(16.0)
                    .children(cells)
                    .into_element()
            })
            .collect();
        children.push(
            rect()
                .direction(Direction::vertical())
                .spacing(2.0)
                .children(rows)
                .into_element(),
        );
    }

    rect()
        .expanded()
        .direction(Direction::vertical())
        .padding(Gaps::new_all(ROOT_PADDING))
        .spacing(12.0)
        .background(SURFACE)
        .color(FG)
        .font_family(font_family())
        .children(children)
        .into_element()
}

/// Render one grid cell into a Freya element. When the cell carries a resolved
/// themed icon ([`GuiCell::icon_path`]), draws the image in a fixed
/// [`ENTRY_ICON_PX`] box followed by the name label; otherwise draws the cell
/// text (which already contains the glyph). Selected rows are bold and
/// accent-colored — no background box — matching the TUI's marker-and-bold style.
fn render_cell(cell: &GuiCell, selected_rgb: (u8, u8, u8)) -> Element {
    let text_node = {
        let node = label().text(cell.text.clone()).font_size(ENTRY_FONT_SIZE);
        if cell.selected {
            node.color(selected_rgb).font_weight(FontWeight::BOLD)
        } else {
            node.color(FG)
        }
    };

    match &cell.icon_path {
        // Themed icon resolved: image box + name label on one row. `ImageViewer`
        // handles async decode, caching, and error states; if the file fails to
        // decode it simply renders nothing, leaving the name legible.
        Some(path) => rect()
            .direction(Direction::horizontal())
            .cross_align(Alignment::Center)
            .spacing(8.0)
            .child(
                ImageViewer::new(path.clone())
                    .width(Size::px(ENTRY_ICON_PX))
                    .height(Size::px(ENTRY_ICON_PX))
                    .into_element(),
            )
            .child(text_node.into_element())
            .into_element(),
        None => text_node.into_element(),
    }
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
        let frame = build_frame(&view, core.config(), IconMode::Glyph);

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
        let frame = build_frame(&view, core.config(), IconMode::Glyph);

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
        let frame = build_frame(&view, core.config(), IconMode::Glyph);

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
        let frame = build_frame(&view, core.config(), IconMode::Glyph);

        // Single app is selected by default, so it carries the marker prefix but
        // no icon glyph.
        assert_eq!(frame.rows[0][0].text, "> Firefox");
    }

    #[test]
    fn build_frame_prefixes_selected_marker_and_pads_others() {
        let mut core = LauncherCore::for_test(apps(2), Config::default());
        core.set_columns(1);
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);

        // Default marker "> " is two columns wide; the unselected row is padded
        // with the same width so names align.
        assert!(frame.rows[0][0].selected);
        assert!(frame.rows[0][0].text.starts_with("> "));
        assert!(!frame.rows[1][0].selected);
        assert!(frame.rows[1][0].text.starts_with("  "));
        assert!(!frame.rows[1][0].text.starts_with("> "));
    }

    #[test]
    fn build_frame_emits_cursor_glyph_when_enabled() {
        let core = LauncherCore::for_test(apps(1), Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
        assert_eq!(frame.cursor.as_deref(), Some("█"));
    }

    #[test]
    fn build_frame_omits_cursor_when_disabled() {
        let cfg = Config {
            ui: crate::config::UiConfig {
                show_cursor: false,
                ..crate::config::UiConfig::default()
            },
            ..Config::default()
        };
        let core = LauncherCore::for_test(apps(1), cfg);
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
        assert_eq!(frame.cursor, None);
    }

    #[test]
    fn resolve_icon_mode_defaults_to_glyph_and_only_themed_opts_in() {
        assert_eq!(resolve_icon_mode(None), IconMode::Glyph);
        assert_eq!(resolve_icon_mode(Some("")), IconMode::Glyph);
        assert_eq!(resolve_icon_mode(Some("glyph")), IconMode::Glyph);
        assert_eq!(resolve_icon_mode(Some("nonsense")), IconMode::Glyph);
        assert_eq!(resolve_icon_mode(Some("themed")), IconMode::Themed);
        assert_eq!(resolve_icon_mode(Some("  Themed  ")), IconMode::Themed);
        assert_eq!(resolve_icon_mode(Some("IMAGES")), IconMode::Themed);
    }

    #[test]
    fn build_frame_themed_falls_back_to_glyph_when_icon_unresolved() {
        // An icon name that cannot exist in any real theme → themed mode must
        // fall back to the glyph so the entry still renders.
        let app = DesktopEntry {
            id: "mystery".into(),
            name: "Mystery".into(),
            icon: "zzz-nonexistent-icon-xyzzy-0000".into(),
            exec: "mystery".into(),
        };
        let core = LauncherCore::for_test(vec![app], Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Themed);

        let cell = &frame.rows[0][0];
        assert_eq!(cell.icon_path, None, "no theme icon should resolve");
        // Unknown app → generic Nerd Font glyph (nf-fa-cube, U+F1B2) must remain.
        assert!(
            cell.text.contains('\u{f1b2}'),
            "unresolved themed icon must keep the glyph, got {:?}",
            cell.text,
        );
    }

    #[test]
    fn build_frame_glyph_mode_never_resolves_an_icon_path() {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        let core = LauncherCore::for_test(vec![app], Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
        assert_eq!(frame.rows[0][0].icon_path, None);
    }

    #[test]
    fn build_frame_themed_with_icons_disabled_draws_no_image() {
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
        let frame = build_frame(&view, core.config(), IconMode::Themed);
        // Icons disabled: no image path, and the name has no glyph prefix.
        assert_eq!(frame.rows[0][0].icon_path, None);
        assert_eq!(frame.rows[0][0].text, "> Firefox");
    }

    #[test]
    fn resolve_font_family_prefers_non_blank_override() {
        assert_eq!(resolve_font_family(None).as_ref(), MONOSPACE_FAMILY);
        assert_eq!(resolve_font_family(Some("   ")).as_ref(), MONOSPACE_FAMILY);
        assert_eq!(
            resolve_font_family(Some("JetBrainsMono Nerd Font")).as_ref(),
            "JetBrainsMono Nerd Font"
        );
    }

    #[test]
    fn columns_for_lists_single_column_and_grids_multiple() {
        // Default config is list mode → one app per row, like the TUI.
        assert_eq!(columns_for(&Config::default()), 1);

        // Grid mode fits more than one column at the default minimum width, and
        // collapses to a single column once the minimum exceeds the window.
        let grid = |min| Config {
            layout: crate::config::LayoutConfig {
                mode: LayoutMode::Grid,
                min_column_width: min,
                ..crate::config::LayoutConfig::default()
            },
            ..Config::default()
        };
        assert!(columns_for(&grid(20)) >= 2);
        assert_eq!(columns_for(&grid(1000)), 1);
    }

    #[test]
    fn build_frame_includes_prompt_with_query() {
        let mut core = LauncherCore::for_test(apps(3), Config::default());
        core.apply(LauncherAction::Insert('a'));
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
        assert_eq!(frame.prompt, "> a");
    }

    #[test]
    fn build_frame_reports_empty_state_message() {
        let core = LauncherCore::for_test(vec![], Config::default());
        let view = core.view();
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
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
        let frame = build_frame(&view, core.config(), IconMode::Glyph);
        assert_eq!(frame.banner_lines, vec!["line one", "line two"]);
    }
}
