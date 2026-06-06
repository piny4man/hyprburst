use std::fmt;
use std::path::{Path, PathBuf};

use ratatui::style::Color;
use serde::Deserialize;

const DEFAULT_BANNER: &str = "\
 _                      _                    _
| |__  _   _ _ __  _ __| |__  _   _ _ __ ___| |_
| '_ \\| | | | '_ \\| '__| '_ \\| | | | '__/ __| __|
| | | | |_| | |_) | |  | |_) | |_| | |  \\__ \\ |_
|_| |_|\\__, | .__/|_|  |_.__/ \\__,_|_|  |___/\\__|
       |___/|_|";

const DEFAULT_PROMPT: &str = "> ";
const DEFAULT_PAGE_SIZE: usize = 10;
pub(crate) const DEFAULT_APP_ID: &str = "hyprburst";
pub(crate) const DEFAULT_WINDOW_WIDTH: u32 = 640;
pub(crate) const DEFAULT_WINDOW_HEIGHT: u32 = 720;
pub(crate) const DEFAULT_FONT_SIZE: f32 = 20.0;
/// Opacity of the background panel painted behind the launcher when the surface
/// is transparent: `1.0` fully hides Hyprland's blur, lower values let more of it
/// through. Dims the blur so text stays legible instead of floating on a raw
/// wallpaper.
pub(crate) const DEFAULT_WINDOW_OPACITY: f32 = 0.85;
pub(crate) const MAX_PADDING: u16 = 32;
pub(crate) const DEFAULT_SELECTED_MARKER: &str = "> ";
pub(crate) const DEFAULT_CURSOR_CHAR: &str = "█";
pub(crate) const DEFAULT_MIN_COLUMN_WIDTH: u16 = 20;
pub(crate) const DEFAULT_PADDING_HORIZONTAL: u16 = 4;
pub(crate) const DEFAULT_PADDING_VERTICAL: u16 = 2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub colors: Colors,
    pub window: WindowConfig,
    pub font: FontConfig,
    pub layout: LayoutConfig,
    pub ui: UiConfig,
}

/// The launcher window: its Wayland app-id (which the Hyprland windowrules
/// match), initial size, and whether the surface is transparent so Hyprland's
/// blur shows through.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    pub app_id: String,
    pub width: u32,
    pub height: u32,
    pub placement: WindowPlacement,
    pub transparent: bool,
    /// Opacity (`0.0`–`1.0`) of the background panel painted behind the launcher
    /// when `transparent = true` — dims Hyprland's blur so text stays legible.
    /// Ignored when `transparent = false` (the surface is already opaque).
    pub opacity: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            app_id: DEFAULT_APP_ID.to_string(),
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            placement: WindowPlacement::default(),
            transparent: true,
            opacity: DEFAULT_WINDOW_OPACITY,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowPlacement {
    /// Floating window sized to the monitor and moved to the top-left. This keeps
    /// blur-friendly overlay behavior without asking the client to fullscreen.
    #[default]
    Fullscreen,
    /// Floating window centered at `window.width` x `window.height`.
    Centered,
}

/// The cell font the window rasterizes glyphs from. `path` is an explicit
/// `.ttf`/`.otf`; when `None` the system monospace (`fc-match`) is used. `size`
/// is the logical pixel height before DPI scaling.
#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    pub path: Option<String>,
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            path: None,
            size: DEFAULT_FONT_SIZE,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutConfig {
    pub mode: LayoutMode,
    pub padding_horizontal: u16,
    pub padding_vertical: u16,
    pub center_banner: bool,
    pub separator: bool,
    pub min_column_width: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::default(),
            padding_horizontal: DEFAULT_PADDING_HORIZONTAL,
            padding_vertical: DEFAULT_PADDING_VERTICAL,
            center_banner: false,
            separator: false,
            min_column_width: DEFAULT_MIN_COLUMN_WIDTH,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LayoutMode {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiConfig {
    pub banner: String,
    pub prompt: String,
    pub page_size: usize,
    pub show_icons: bool,
    pub selected_marker: String,
    pub cursor_char: String,
    pub show_cursor: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub banner: Color,
    pub prompt: Color,
    pub selected: Color,
    /// Background highlight bar drawn behind the selected row.
    pub selected_bg: Color,
    pub empty: Color,
    /// Window background — painted opaque when `window.transparent = false`, and
    /// as the dimming panel (at `window.opacity`) when `transparent = true`.
    pub background: Color,
    /// Default text color for the unstyled (Reset) launcher text.
    pub foreground: Color,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            banner: DEFAULT_BANNER.to_string(),
            prompt: DEFAULT_PROMPT.to_string(),
            page_size: DEFAULT_PAGE_SIZE,
            show_icons: true,
            selected_marker: DEFAULT_SELECTED_MARKER.to_string(),
            cursor_char: DEFAULT_CURSOR_CHAR.to_string(),
            show_cursor: true,
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        // A modern dark, violet-tinted palette (no pure black/white), all
        // overridable in `[colors]`.
        Self {
            banner: Color::Rgb(0xc6, 0xa0, 0xf6),      // lavender
            prompt: Color::Rgb(0x8a, 0xad, 0xf4),      // periwinkle
            selected: Color::Rgb(0xf2, 0xd5, 0xff),    // bright violet-white
            selected_bg: Color::Rgb(0x3a, 0x2e, 0x5a), // muted violet bar
            empty: Color::Rgb(0x9a, 0x8c, 0xb5),       // muted lilac
            background: Color::Rgb(0x1a, 0x1b, 0x26),  // dark slate-violet
            foreground: Color::Rgb(0xc8, 0xcc, 0xe0),  // soft light
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
    Validation(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "failed to read config file: {}", err),
            Self::Parse(msg) => write!(f, "failed to parse config: {}", msg),
            Self::Validation(msg) => write!(f, "invalid config value: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&default_path())
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let (cfg, warnings) = Self::from_toml_str_validating(&contents)?;
                for warning in warnings {
                    eprintln!("hyprburst config warning: {}", warning);
                }
                Ok(cfg)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError::Io(err)),
        }
    }

    pub fn from_toml_str(contents: &str) -> Result<Self, ConfigError> {
        Self::from_toml_str_validating(contents).map(|(cfg, _)| cfg)
    }

    /// Parse config and return `(Config, warnings)`. Warnings describe
    /// recoverable issues (e.g. an invalid terminal flag template) that fall
    /// back to defaults rather than aborting the load.
    pub fn from_toml_str_validating(contents: &str) -> Result<(Self, Vec<String>), ConfigError> {
        let raw: RawConfig = toml::from_str(contents).map_err(|e| {
            let msg = e.message().to_string();
            if msg.contains("terminal") {
                ConfigError::Parse(format!(
                    "{msg}\n\nThe [terminal] section was removed in hyprburst 0.5: the launcher \
                     now opens its own window instead of re-execing a terminal. Move \
                     `terminal.class` to `window.app_id`, drop `terminal.preferred`/`terminal.flags`, \
                     and see config.example.toml for the new [window]/[font] options."
                ))
            } else {
                ConfigError::Parse(msg)
            }
        })?;
        raw.into_config()
    }
}

pub fn default_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("hyprburst").join("config.toml");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".config/hyprburst")
            .join("config.toml");
    }
    PathBuf::from("hyprburst.toml")
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    colors: RawColors,
    window: RawWindow,
    font: RawFont,
    layout: RawLayout,
    ui: RawUi,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawUi {
    banner: Option<String>,
    prompt: Option<String>,
    page_size: Option<usize>,
    show_icons: Option<bool>,
    selected_marker: Option<String>,
    cursor_char: Option<String>,
    show_cursor: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawLayout {
    mode: Option<String>,
    padding_horizontal: Option<u16>,
    padding_vertical: Option<u16>,
    center_banner: Option<bool>,
    separator: Option<bool>,
    min_column_width: Option<u16>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawColors {
    banner: Option<String>,
    prompt: Option<String>,
    selected: Option<String>,
    selected_bg: Option<String>,
    empty: Option<String>,
    background: Option<String>,
    foreground: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawWindow {
    app_id: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    placement: Option<String>,
    transparent: Option<bool>,
    opacity: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawFont {
    path: Option<String>,
    size: Option<f32>,
}

impl RawConfig {
    fn into_config(self) -> Result<(Config, Vec<String>), ConfigError> {
        let defaults = Config::default();
        let mut warnings = Vec::new();

        let cfg = Config {
            colors: Colors {
                banner: resolve_color(self.colors.banner, "colors.banner", defaults.colors.banner)?,
                prompt: resolve_color(self.colors.prompt, "colors.prompt", defaults.colors.prompt)?,
                selected: resolve_color(
                    self.colors.selected,
                    "colors.selected",
                    defaults.colors.selected,
                )?,
                selected_bg: resolve_color(
                    self.colors.selected_bg,
                    "colors.selected_bg",
                    defaults.colors.selected_bg,
                )?,
                empty: resolve_color(self.colors.empty, "colors.empty", defaults.colors.empty)?,
                background: resolve_color(
                    self.colors.background,
                    "colors.background",
                    defaults.colors.background,
                )?,
                foreground: resolve_color(
                    self.colors.foreground,
                    "colors.foreground",
                    defaults.colors.foreground,
                )?,
            },
            window: self.window.into_config()?,
            font: self.font.into_config()?,
            layout: self.layout.into_config(&mut warnings)?,
            ui: self.ui.into_config(&mut warnings)?,
        };

        Ok((cfg, warnings))
    }
}

impl RawWindow {
    fn into_config(self) -> Result<WindowConfig, ConfigError> {
        let defaults = WindowConfig::default();

        let app_id = match self.app_id {
            Some(c) if c.trim().is_empty() => {
                return Err(ConfigError::Validation(
                    "window.app_id must not be empty".to_string(),
                ));
            }
            Some(c) => c,
            None => defaults.app_id,
        };

        let width = match self.width {
            Some(0) => {
                return Err(ConfigError::Validation(
                    "window.width must be at least 1".to_string(),
                ));
            }
            Some(w) => w,
            None => defaults.width,
        };

        let height = match self.height {
            Some(0) => {
                return Err(ConfigError::Validation(
                    "window.height must be at least 1".to_string(),
                ));
            }
            Some(h) => h,
            None => defaults.height,
        };

        let opacity = match self.opacity {
            Some(o) if !(o.is_finite() && (0.0..=1.0).contains(&o)) => {
                return Err(ConfigError::Validation(
                    "window.opacity must be between 0.0 and 1.0".to_string(),
                ));
            }
            Some(o) => o,
            None => defaults.opacity,
        };

        let placement = match self.placement.as_deref() {
            None => defaults.placement,
            Some("fullscreen") => WindowPlacement::Fullscreen,
            Some("centered") => WindowPlacement::Centered,
            Some(other) => {
                return Err(ConfigError::Validation(format!(
                    "window.placement must be \"fullscreen\" or \"centered\", got {:?}",
                    other
                )));
            }
        };

        Ok(WindowConfig {
            app_id,
            width,
            height,
            placement,
            transparent: self.transparent.unwrap_or(defaults.transparent),
            opacity,
        })
    }
}

impl RawFont {
    fn into_config(self) -> Result<FontConfig, ConfigError> {
        let defaults = FontConfig::default();

        let size = match self.size {
            Some(s) if !(s.is_finite() && s > 0.0) => {
                return Err(ConfigError::Validation(
                    "font.size must be a positive number".to_string(),
                ));
            }
            Some(s) => s,
            None => defaults.size,
        };

        let path = match self.path {
            Some(p) if p.trim().is_empty() => None,
            other => other,
        };

        Ok(FontConfig { path, size })
    }
}

impl RawUi {
    fn into_config(self, warnings: &mut Vec<String>) -> Result<UiConfig, ConfigError> {
        let defaults = UiConfig::default();

        if let Some(size) = self.page_size
            && size == 0
        {
            return Err(ConfigError::Validation(
                "ui.page_size must be at least 1".to_string(),
            ));
        }

        let selected_marker = match self.selected_marker {
            Some(s) if s.is_empty() => {
                warnings.push(format!(
                    "ui.selected_marker is empty; falling back to default {:?}",
                    defaults.selected_marker
                ));
                defaults.selected_marker.clone()
            }
            Some(s) => s,
            None => defaults.selected_marker.clone(),
        };

        let cursor_char = match self.cursor_char {
            Some(s) if s.chars().count() != 1 => {
                warnings.push(format!(
                    "ui.cursor_char must be exactly one character, got {:?}; falling back to default {:?}",
                    s, defaults.cursor_char
                ));
                defaults.cursor_char.clone()
            }
            Some(s) => s,
            None => defaults.cursor_char.clone(),
        };

        Ok(UiConfig {
            banner: self.banner.unwrap_or(defaults.banner),
            prompt: self.prompt.unwrap_or(defaults.prompt),
            page_size: self.page_size.unwrap_or(defaults.page_size),
            show_icons: self.show_icons.unwrap_or(defaults.show_icons),
            selected_marker,
            cursor_char,
            show_cursor: self.show_cursor.unwrap_or(defaults.show_cursor),
        })
    }
}

impl RawLayout {
    fn into_config(self, warnings: &mut Vec<String>) -> Result<LayoutConfig, ConfigError> {
        let defaults = LayoutConfig::default();
        let padding_horizontal = resolve_padding(
            self.padding_horizontal,
            "layout.padding_horizontal",
            warnings,
        )
        .unwrap_or(defaults.padding_horizontal);
        let padding_vertical =
            resolve_padding(self.padding_vertical, "layout.padding_vertical", warnings)
                .unwrap_or(defaults.padding_vertical);

        let mode = match self.mode.as_deref() {
            None => defaults.mode,
            Some("list") => LayoutMode::List,
            Some("grid") => LayoutMode::Grid,
            Some(other) => {
                return Err(ConfigError::Validation(format!(
                    "layout.mode must be \"list\" or \"grid\", got {:?}",
                    other
                )));
            }
        };

        let min_column_width = match self.min_column_width {
            None => defaults.min_column_width,
            Some(0) => {
                return Err(ConfigError::Validation(
                    "layout.min_column_width must be at least 1".to_string(),
                ));
            }
            Some(v) => v,
        };

        Ok(LayoutConfig {
            mode,
            padding_horizontal,
            padding_vertical,
            center_banner: self.center_banner.unwrap_or(defaults.center_banner),
            separator: self.separator.unwrap_or(defaults.separator),
            min_column_width,
        })
    }
}

fn resolve_padding(value: Option<u16>, field: &str, warnings: &mut Vec<String>) -> Option<u16> {
    match value {
        Some(v) if v > MAX_PADDING => {
            warnings.push(format!(
                "{} = {} exceeds cap of {}; falling back to default",
                field, v, MAX_PADDING
            ));
            None
        }
        other => other,
    }
}

fn resolve_color(value: Option<String>, field: &str, default: Color) -> Result<Color, ConfigError> {
    match value {
        Some(raw) => parse_color(&raw).map_err(|msg| {
            ConfigError::Validation(format!("{} is not a valid color ({})", field, msg))
        }),
        None => Ok(default),
    }
}

fn parse_color(input: &str) -> Result<Color, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("color cannot be empty".to_string());
    }

    if let Some(hex) = trimmed.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(format!(
                "expected 6-digit hex code like #RRGGBB, got {:?}",
                input
            ));
        }
        let r = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| format!("bad hex red in {:?}", input))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| format!("bad hex green in {:?}", input))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| format!("bad hex blue in {:?}", input))?;
        return Ok(Color::Rgb(r, g, b));
    }

    let normalized = trimmed.to_lowercase().replace(['-', '_'], "");
    match normalized.as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" => Ok(Color::Gray),
        "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        "reset" => Ok(Color::Reset),
        _ => Err(format!(
            "unknown color name {:?}; use a named color or #RRGGBB",
            input
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "hyprburst-config-test-{}-{}",
                std::process::id(),
                n
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn default_has_sensible_values() {
        let cfg = Config::default();
        assert!(!cfg.ui.banner.is_empty());
        assert_eq!(cfg.ui.prompt, "> ");
        assert_eq!(cfg.ui.page_size, 10);
        // The default palette is a violet-tinted dark theme (all RGB).
        assert!(matches!(cfg.colors.prompt, Color::Rgb(..)));
        assert!(matches!(cfg.colors.selected, Color::Rgb(..)));
        assert!(matches!(cfg.colors.selected_bg, Color::Rgb(..)));
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = TempDir::new();
        let path = dir.path().join("missing.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn empty_toml_returns_defaults() {
        let cfg = Config::from_toml_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn full_toml_overrides_all_fields() {
        let toml = r#"
[ui]
banner = "hello"
prompt = "$ "
page_size = 5

[colors]
banner = "red"
prompt = "blue"
selected = "green"
empty = "white"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.ui.banner, "hello");
        assert_eq!(cfg.ui.prompt, "$ ");
        assert_eq!(cfg.ui.page_size, 5);
        assert_eq!(cfg.colors.banner, Color::Red);
        assert_eq!(cfg.colors.prompt, Color::Blue);
        assert_eq!(cfg.colors.selected, Color::Green);
        assert_eq!(cfg.colors.empty, Color::White);
    }

    #[test]
    fn partial_toml_fills_missing_with_defaults() {
        let toml = r#"
[ui]
prompt = "% "
[colors]
banner = "red"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let defaults = Config::default();
        assert_eq!(cfg.ui.prompt, "% ");
        assert_eq!(cfg.ui.banner, defaults.ui.banner);
        assert_eq!(cfg.ui.page_size, defaults.ui.page_size);
        assert_eq!(cfg.colors.banner, Color::Red);
        assert_eq!(cfg.colors.prompt, defaults.colors.prompt);
        assert_eq!(cfg.colors.selected, defaults.colors.selected);
    }

    #[test]
    fn hex_color_parses_to_rgb() {
        let cfg = Config::from_toml_str(
            r##"[colors]
prompt = "#ff8800"
"##,
        )
        .unwrap();
        assert_eq!(cfg.colors.prompt, Color::Rgb(0xff, 0x88, 0x00));
    }

    #[test]
    fn named_color_variants_are_case_insensitive() {
        let cfg = Config::from_toml_str(
            r#"[colors]
banner = "MAGENTA"
prompt = "Cyan""#,
        )
        .unwrap();
        assert_eq!(cfg.colors.banner, Color::Magenta);
        assert_eq!(cfg.colors.prompt, Color::Cyan);
    }

    #[test]
    fn light_colors_accept_dash_or_underscore() {
        let cfg = Config::from_toml_str(
            r#"[colors]
banner = "light-red"
prompt = "light_blue""#,
        )
        .unwrap();
        assert_eq!(cfg.colors.banner, Color::LightRed);
        assert_eq!(cfg.colors.prompt, Color::LightBlue);
    }

    #[test]
    fn invalid_toml_returns_parse_error() {
        let err = Config::from_toml_str("banner = ").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
        let message = format!("{}", err);
        assert!(message.contains("parse"), "got: {}", message);
    }

    #[test]
    fn unknown_color_returns_validation_error_with_field_name() {
        let toml = r#"[colors]
prompt = "notacolor""#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        let msg = format!("{}", err);
        assert!(msg.contains("colors.prompt"), "got: {}", msg);
        assert!(msg.contains("notacolor"), "got: {}", msg);
    }

    #[test]
    fn malformed_hex_returns_validation_error() {
        let toml = r##"[colors]
selected = "#xyzxyz"
"##;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn short_hex_returns_validation_error() {
        let toml = r##"[colors]
empty = "#fff"
"##;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn zero_page_size_is_rejected() {
        let toml = r#"
[ui]
page_size = 0
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn unknown_top_level_field_rejected() {
        let err = Config::from_toml_str("nope = 1").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn unknown_color_field_rejected() {
        let toml = r#"[colors]
sparkle = "red""#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn default_window_has_sensible_values() {
        let cfg = Config::default();
        assert_eq!(cfg.window.app_id, "hyprburst");
        assert_eq!(cfg.window.width, 640);
        assert_eq!(cfg.window.height, 720);
        assert_eq!(cfg.window.placement, WindowPlacement::Fullscreen);
        assert!(cfg.window.transparent);
        assert_eq!(cfg.window.opacity, 0.85);
    }

    #[test]
    fn window_opacity_round_trips_and_is_range_checked() {
        let cfg = Config::from_toml_str("[window]\nopacity = 0.5\n").unwrap();
        assert_eq!(cfg.window.opacity, 0.5);

        for bad in ["1.5", "-0.1"] {
            let err = Config::from_toml_str(&format!("[window]\nopacity = {bad}\n")).unwrap_err();
            assert!(matches!(err, ConfigError::Validation(_)), "opacity {bad}");
        }
    }

    #[test]
    fn window_placement_round_trips() {
        for (raw, expected) in [
            ("fullscreen", WindowPlacement::Fullscreen),
            ("centered", WindowPlacement::Centered),
        ] {
            let cfg = Config::from_toml_str(&format!("[window]\nplacement = \"{raw}\"\n")).unwrap();
            assert_eq!(cfg.window.placement, expected);
        }
    }

    #[test]
    fn invalid_window_placement_rejected() {
        let err = Config::from_toml_str("[window]\nplacement = \"overlay\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn selected_bg_color_parses_and_defaults() {
        let cfg = Config::from_toml_str("[colors]\nselected_bg = \"#102030\"\n").unwrap();
        assert_eq!(cfg.colors.selected_bg, Color::Rgb(0x10, 0x20, 0x30));
        // Omitted → default violet bar.
        let cfg = Config::from_toml_str("").unwrap();
        assert_eq!(cfg.colors.selected_bg, Config::default().colors.selected_bg);
    }

    #[test]
    fn full_window_section_round_trips() {
        let toml = r#"
[window]
app_id = "my-launcher"
width = 800
height = 600
transparent = false
placement = "centered"
opacity = 0.75
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.window.app_id, "my-launcher");
        assert_eq!(cfg.window.width, 800);
        assert_eq!(cfg.window.height, 600);
        assert_eq!(cfg.window.placement, WindowPlacement::Centered);
        assert!(!cfg.window.transparent);
        assert_eq!(cfg.window.opacity, 0.75);
    }

    #[test]
    fn partial_window_section_uses_defaults() {
        let toml = r#"
[window]
app_id = "custom"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let defaults = WindowConfig::default();
        assert_eq!(cfg.window.app_id, "custom");
        assert_eq!(cfg.window.width, defaults.width);
        assert_eq!(cfg.window.transparent, defaults.transparent);
    }

    #[test]
    fn empty_window_app_id_rejected() {
        let err = Config::from_toml_str("[window]\napp_id = \"\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn zero_window_size_rejected() {
        let err = Config::from_toml_str("[window]\nwidth = 0\n").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn unknown_window_field_rejected() {
        let err = Config::from_toml_str("[window]\nmystery = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn default_font_has_sensible_values() {
        let cfg = Config::default();
        assert_eq!(cfg.font.path, None);
        assert_eq!(cfg.font.size, 20.0);
    }

    #[test]
    fn full_font_section_round_trips() {
        let toml = r#"
[font]
path = "/usr/share/fonts/TTF/MyFont.ttf"
size = 18.0
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(
            cfg.font.path.as_deref(),
            Some("/usr/share/fonts/TTF/MyFont.ttf")
        );
        assert_eq!(cfg.font.size, 18.0);
    }

    #[test]
    fn empty_font_path_becomes_none() {
        let cfg = Config::from_toml_str("[font]\npath = \"\"\n").unwrap();
        assert_eq!(cfg.font.path, None);
    }

    #[test]
    fn non_positive_font_size_rejected() {
        let err = Config::from_toml_str("[font]\nsize = 0.0\n").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn unknown_font_field_rejected() {
        let err = Config::from_toml_str("[font]\nweight = 700\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn default_colors_have_background_and_foreground() {
        let cfg = Config::default();
        // Dark, violet-tinted defaults (no pure black/white).
        assert!(matches!(cfg.colors.background, Color::Rgb(..)));
        assert!(matches!(cfg.colors.foreground, Color::Rgb(..)));
    }

    #[test]
    fn background_and_foreground_colors_round_trip() {
        let toml = r##"
[colors]
background = "#1e1e2e"
foreground = "white"
"##;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.colors.background, Color::Rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(cfg.colors.foreground, Color::White);
    }

    #[test]
    fn removed_terminal_section_errors_with_migration_hint() {
        let toml = r#"
[terminal]
preferred = ["rio"]
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        let msg = format!("{}", err);
        assert!(matches!(err, ConfigError::Parse(_)));
        assert!(
            msg.contains("window.app_id"),
            "migration hint should point at window.app_id, got: {}",
            msg
        );
    }

    #[test]
    fn load_from_reads_file() {
        let dir = TempDir::new();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[ui]\nprompt = \"$ \"\n").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.ui.prompt, "$ ");
    }

    #[test]
    fn load_from_surfaces_parse_errors() {
        let dir = TempDir::new();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[ui]\nbanner = ").unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn default_path_uses_xdg_config_home() {
        // Snapshot original vars to restore afterward.
        let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let original_home = std::env::var("HOME").ok();

        // SAFETY: tests run single-threaded when acquiring the lock below.
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", "/custom/config");
        }
        let path = default_path();
        assert_eq!(path, PathBuf::from("/custom/config/hyprburst/config.toml"));

        unsafe {
            match original_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match original_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn default_path_falls_back_to_home() {
        let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let original_home = std::env::var("HOME").ok();

        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("HOME", "/home/alice");
        }
        let path = default_path();
        assert_eq!(
            path,
            PathBuf::from("/home/alice/.config/hyprburst/config.toml")
        );

        unsafe {
            match original_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match original_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn default_layout_uses_breathing_room() {
        let cfg = Config::default();
        assert_eq!(cfg.layout.padding_horizontal, 4);
        assert_eq!(cfg.layout.padding_vertical, 2);
        assert!(!cfg.layout.center_banner);
        assert!(!cfg.layout.separator);
    }

    #[test]
    fn zero_padding_layout_values_override_defaults() {
        let toml = r#"
[layout]
padding_horizontal = 0
padding_vertical = 0
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.layout.padding_horizontal, 0);
        assert_eq!(cfg.layout.padding_vertical, 0);
    }

    #[test]
    fn full_layout_section_round_trips() {
        let toml = r#"
[layout]
padding_horizontal = 4
padding_vertical = 2
center_banner = true
separator = true
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.layout.padding_horizontal, 4);
        assert_eq!(cfg.layout.padding_vertical, 2);
        assert!(cfg.layout.center_banner);
        assert!(cfg.layout.separator);
    }

    #[test]
    fn partial_layout_fills_missing_with_defaults() {
        let toml = r#"
[layout]
center_banner = true
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let defaults = LayoutConfig::default();
        assert!(cfg.layout.center_banner);
        assert_eq!(cfg.layout.padding_horizontal, defaults.padding_horizontal);
        assert_eq!(cfg.layout.padding_vertical, defaults.padding_vertical);
        assert_eq!(cfg.layout.separator, defaults.separator);
    }

    #[test]
    fn unknown_layout_field_rejected() {
        let toml = r#"
[layout]
mystery = true
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn padding_above_cap_warns_and_falls_back_to_default() {
        let toml = r#"
[layout]
padding_horizontal = 9999
padding_vertical = 33
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        let defaults = LayoutConfig::default();
        assert_eq!(cfg.layout.padding_horizontal, defaults.padding_horizontal);
        assert_eq!(cfg.layout.padding_vertical, defaults.padding_vertical);
        assert_eq!(warnings.len(), 2, "warnings: {:?}", warnings);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("padding_horizontal") && w.contains("9999"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("padding_vertical") && w.contains("33"))
        );
    }

    #[test]
    fn default_ui_preserves_today_render() {
        let cfg = Config::default();
        assert!(cfg.ui.show_icons);
        assert_eq!(cfg.ui.selected_marker, "> ");
        assert_eq!(cfg.ui.cursor_char, "█");
        assert!(cfg.ui.show_cursor);
    }

    #[test]
    fn full_ui_section_round_trips() {
        let toml = r#"
[ui]
show_icons = false
selected_marker = "» "
cursor_char = "▏"
show_cursor = false
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert!(!cfg.ui.show_icons);
        assert_eq!(cfg.ui.selected_marker, "» ");
        assert_eq!(cfg.ui.cursor_char, "▏");
        assert!(!cfg.ui.show_cursor);
    }

    #[test]
    fn partial_ui_section_fills_missing_with_defaults() {
        let toml = r#"
[ui]
show_icons = false
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let defaults = UiConfig::default();
        assert!(!cfg.ui.show_icons);
        assert_eq!(cfg.ui.selected_marker, defaults.selected_marker);
        assert_eq!(cfg.ui.cursor_char, defaults.cursor_char);
        assert_eq!(cfg.ui.show_cursor, defaults.show_cursor);
    }

    #[test]
    fn unknown_ui_field_rejected() {
        let toml = r#"
[ui]
mystery = true
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn multi_grapheme_cursor_char_warns_and_falls_back() {
        let toml = r#"
[ui]
cursor_char = "██"
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        let defaults = UiConfig::default();
        assert_eq!(cfg.ui.cursor_char, defaults.cursor_char);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("cursor_char") && warnings[0].contains("██"),
            "unexpected warning: {}",
            warnings[0]
        );
    }

    #[test]
    fn empty_cursor_char_warns_and_falls_back() {
        let toml = r#"
[ui]
cursor_char = ""
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        let defaults = UiConfig::default();
        assert_eq!(cfg.ui.cursor_char, defaults.cursor_char);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("cursor_char"));
    }

    #[test]
    fn empty_selected_marker_warns_and_falls_back() {
        let toml = r#"
[ui]
selected_marker = ""
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        let defaults = UiConfig::default();
        assert_eq!(cfg.ui.selected_marker, defaults.selected_marker);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("selected_marker"));
    }

    #[test]
    fn valid_ui_section_emits_no_warnings() {
        let toml = r#"
[ui]
show_icons = false
selected_marker = "▶ "
cursor_char = "_"
show_cursor = true
"#;
        let (_, warnings) = Config::from_toml_str_validating(toml).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn default_layout_mode_is_list() {
        let cfg = Config::default();
        assert_eq!(cfg.layout.mode, LayoutMode::List);
        assert_eq!(cfg.layout.min_column_width, DEFAULT_MIN_COLUMN_WIDTH);
    }

    #[test]
    fn grid_layout_section_round_trips() {
        let toml = r#"
[layout]
mode = "grid"
min_column_width = 24
padding_horizontal = 2
padding_vertical = 1
center_banner = true
separator = true
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.layout.mode, LayoutMode::Grid);
        assert_eq!(cfg.layout.min_column_width, 24);
        assert_eq!(cfg.layout.padding_horizontal, 2);
        assert_eq!(cfg.layout.padding_vertical, 1);
        assert!(cfg.layout.center_banner);
        assert!(cfg.layout.separator);
    }

    #[test]
    fn explicit_list_mode_parses() {
        let toml = r#"
[layout]
mode = "list"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.layout.mode, LayoutMode::List);
    }

    #[test]
    fn invalid_layout_mode_rejected() {
        let toml = r#"
[layout]
mode = "masonry"
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn zero_min_column_width_rejected() {
        let toml = r#"
[layout]
mode = "grid"
min_column_width = 0
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn padding_at_cap_is_accepted_without_warning() {
        let toml = r#"
[layout]
padding_horizontal = 32
padding_vertical = 32
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        assert_eq!(cfg.layout.padding_horizontal, 32);
        assert_eq!(cfg.layout.padding_vertical, 32);
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
    }
}
