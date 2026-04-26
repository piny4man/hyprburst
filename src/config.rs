use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use ratatui::style::Color;
use serde::Deserialize;

const DEFAULT_BANNER: &str = "\
 _                    _
| |__  _   _ _ __ ___| |_
| '_ \\| | | | '__/ __| __|
| |_) | |_| | |  \\__ \\ |_
|_.__/ \\__,_|_|  |___/\\__|";

const DEFAULT_PROMPT: &str = "> ";
const DEFAULT_PAGE_SIZE: usize = 10;
pub(crate) const MAX_PADDING: u16 = 32;
pub(crate) const DEFAULT_SELECTED_MARKER: &str = "> ";
pub(crate) const DEFAULT_CURSOR_CHAR: &str = "█";
pub(crate) const DEFAULT_MIN_COLUMN_WIDTH: u16 = 20;
pub(crate) const DEFAULT_PADDING_HORIZONTAL: u16 = 4;
pub(crate) const DEFAULT_PADDING_VERTICAL: u16 = 2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub colors: Colors,
    pub terminal: TerminalConfig,
    pub layout: LayoutConfig,
    pub ui: UiConfig,
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
    pub loading_polish: bool,
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
    pub empty: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalConfig {
    pub preferred: Vec<String>,
    pub class: String,
    pub flags: BTreeMap<String, Vec<String>>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            banner: DEFAULT_BANNER.to_string(),
            prompt: DEFAULT_PROMPT.to_string(),
            page_size: DEFAULT_PAGE_SIZE,
            loading_polish: true,
            show_icons: true,
            selected_marker: DEFAULT_SELECTED_MARKER.to_string(),
            cursor_char: DEFAULT_CURSOR_CHAR.to_string(),
            show_cursor: true,
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            preferred: Vec::new(),
            class: "burst".to_string(),
            flags: builtin_flags(),
        }
    }
}

pub(crate) fn builtin_flags() -> BTreeMap<String, Vec<String>> {
    let entries: &[(&str, &[&str])] = &[
        ("alacritty", &["--class={class}", "-e", "{cmd}"]),
        ("wezterm", &["start", "--class={class}", "--", "{cmd}"]),
        ("ghostty", &["--class={class}", "-e", "{cmd}"]),
        ("kitty", &["--class={class}", "{cmd}"]),
        ("foot", &["--app-id={class}", "{cmd}"]),
        ("rio", &["--title={class}", "-e", "{cmd}"]),
    ];
    entries
        .iter()
        .map(|(name, args)| {
            (
                (*name).to_string(),
                args.iter().map(|s| (*s).to_string()).collect(),
            )
        })
        .collect()
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            banner: Color::Magenta,
            prompt: Color::Cyan,
            selected: Color::Yellow,
            empty: Color::Yellow,
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
                    eprintln!("burst config warning: {}", warning);
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
        let raw: RawConfig =
            toml::from_str(contents).map_err(|e| ConfigError::Parse(e.message().to_string()))?;
        raw.into_config()
    }
}

pub fn default_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("burst").join("config.toml");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".config/burst")
            .join("config.toml");
    }
    PathBuf::from("burst.toml")
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    colors: RawColors,
    terminal: RawTerminal,
    layout: RawLayout,
    ui: RawUi,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawUi {
    banner: Option<String>,
    prompt: Option<String>,
    page_size: Option<usize>,
    loading_polish: Option<bool>,
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
    empty: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTerminal {
    preferred: Option<Vec<String>>,
    class: Option<String>,
    flags: Option<BTreeMap<String, RawFlagEntry>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFlagEntry {
    args: Vec<String>,
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
                empty: resolve_color(self.colors.empty, "colors.empty", defaults.colors.empty)?,
            },
            terminal: self.terminal.into_config(&mut warnings)?,
            layout: self.layout.into_config(&mut warnings)?,
            ui: self.ui.into_config(&mut warnings)?,
        };

        Ok((cfg, warnings))
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
            loading_polish: self.loading_polish.unwrap_or(defaults.loading_polish),
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

impl RawTerminal {
    fn into_config(self, warnings: &mut Vec<String>) -> Result<TerminalConfig, ConfigError> {
        let defaults = TerminalConfig::default();

        let class = match self.class {
            Some(c) if c.trim().is_empty() => {
                return Err(ConfigError::Validation(
                    "terminal.class must not be empty".to_string(),
                ));
            }
            Some(c) => c,
            None => defaults.class,
        };

        let mut flags = defaults.flags;
        if let Some(custom) = self.flags {
            for (name, entry) in custom {
                if !entry.args.iter().any(|s| s.contains("{cmd}")) {
                    warnings.push(format!(
                        "terminal.flags.{} args missing required {{cmd}} placeholder; falling back to built-in defaults",
                        name
                    ));
                    continue;
                }
                flags.insert(name, entry.args);
            }
        }

        Ok(TerminalConfig {
            preferred: self.preferred.unwrap_or(defaults.preferred),
            class,
            flags,
        })
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
                "burst-config-test-{}-{}",
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
        assert!(cfg.ui.loading_polish);
        assert_eq!(cfg.colors.prompt, Color::Cyan);
        assert_eq!(cfg.colors.selected, Color::Yellow);
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
        assert!(cfg.ui.loading_polish);
        assert_eq!(cfg.colors.banner, Color::Red);
        assert_eq!(cfg.colors.prompt, Color::Blue);
        assert_eq!(cfg.colors.selected, Color::Green);
        assert_eq!(cfg.colors.empty, Color::White);
    }

    #[test]
    fn loading_polish_can_be_disabled() {
        let cfg = Config::from_toml_str(
            r#"[ui]
loading_polish = false
"#,
        )
        .unwrap();

        assert!(!cfg.ui.loading_polish);
    }

    #[test]
    fn loading_polish_does_not_reset_layout_padding() {
        let cfg = Config::from_toml_str(
            r#"[layout]
padding_horizontal = 6
padding_vertical = 3

[ui]
loading_polish = false
"#,
        )
        .unwrap();

        assert!(!cfg.ui.loading_polish);
        assert_eq!(cfg.layout.padding_horizontal, 6);
        assert_eq!(cfg.layout.padding_vertical, 3);
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
    fn default_terminal_has_builtin_flag_table() {
        let cfg = Config::default();
        assert!(cfg.terminal.preferred.is_empty());
        assert_eq!(cfg.terminal.class, "burst");
        for name in ["alacritty", "wezterm", "ghostty", "kitty", "foot", "rio"] {
            assert!(
                cfg.terminal.flags.contains_key(name),
                "missing builtin flag entry for {}",
                name
            );
        }
    }

    #[test]
    fn full_terminal_section_round_trips() {
        let toml = r#"
[terminal]
preferred = ["rio", "ghostty"]
class = "my-launcher"

[terminal.flags.rio]
args = ["--title={class}", "-e", "{cmd}"]

[terminal.flags.cosmic-term]
args = ["--class={class}", "-e", "{cmd}"]
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.terminal.preferred, vec!["rio", "ghostty"]);
        assert_eq!(cfg.terminal.class, "my-launcher");
        assert_eq!(
            cfg.terminal.flags.get("cosmic-term").unwrap(),
            &vec![
                "--class={class}".to_string(),
                "-e".to_string(),
                "{cmd}".to_string()
            ]
        );
        // Built-in entries we didn't override remain present.
        assert!(cfg.terminal.flags.contains_key("kitty"));
    }

    #[test]
    fn partial_terminal_section_uses_defaults() {
        let toml = r#"
[terminal]
preferred = ["rio"]
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.terminal.preferred, vec!["rio"]);
        assert_eq!(cfg.terminal.class, "burst");
        // Built-in flag table is preserved when [terminal.flags] is omitted.
        assert!(cfg.terminal.flags.contains_key("alacritty"));
    }

    #[test]
    fn unknown_terminal_field_rejected() {
        let toml = r#"
[terminal]
preferred = ["rio"]
mystery = "x"
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn unknown_terminal_flags_field_rejected() {
        let toml = r#"
[terminal.flags.rio]
args = ["{cmd}"]
extra = "bad"
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn empty_terminal_class_rejected() {
        let toml = r#"
[terminal]
class = ""
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn invalid_flag_template_warns_and_falls_back_to_default() {
        let toml = r#"
[terminal.flags.alacritty]
args = ["--class={class}", "--no-cmd-here"]
"#;
        let (cfg, warnings) = Config::from_toml_str_validating(toml).unwrap();
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(
            warnings[0].contains("alacritty") && warnings[0].contains("{cmd}"),
            "unexpected warning: {}",
            warnings[0]
        );
        // Built-in alacritty entry preserved instead of the broken override.
        let defaults = TerminalConfig::default();
        assert_eq!(
            cfg.terminal.flags.get("alacritty"),
            defaults.flags.get("alacritty")
        );
    }

    #[test]
    fn valid_flag_template_round_trip_emits_no_warnings() {
        let toml = r#"
[terminal.flags.alacritty]
args = ["--class={class}", "-e", "{cmd}"]
"#;
        let (_, warnings) = Config::from_toml_str_validating(toml).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
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
        assert_eq!(path, PathBuf::from("/custom/config/burst/config.toml"));

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
        assert_eq!(path, PathBuf::from("/home/alice/.config/burst/config.toml"));

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
