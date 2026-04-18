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

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub banner: String,
    pub prompt: String,
    pub page_size: usize,
    pub colors: Colors,
    pub window: Window,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub banner: Color,
    pub prompt: Color,
    pub selected: Color,
    pub empty: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fullscreen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            banner: DEFAULT_BANNER.to_string(),
            prompt: DEFAULT_PROMPT.to_string(),
            page_size: DEFAULT_PAGE_SIZE,
            colors: Colors::default(),
            window: Window::default(),
        }
    }
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            fullscreen: true,
        }
    }
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
            Ok(contents) => Self::from_toml_str(&contents),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError::Io(err)),
        }
    }

    pub fn from_toml_str(contents: &str) -> Result<Self, ConfigError> {
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
    banner: Option<String>,
    prompt: Option<String>,
    page_size: Option<usize>,
    colors: RawColors,
    window: RawWindow,
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
struct RawWindow {
    width: Option<u32>,
    height: Option<u32>,
    fullscreen: Option<bool>,
}

impl RawConfig {
    fn into_config(self) -> Result<Config, ConfigError> {
        let defaults = Config::default();

        if let Some(size) = self.page_size
            && size == 0
        {
            return Err(ConfigError::Validation(
                "page_size must be at least 1".to_string(),
            ));
        }

        Ok(Config {
            banner: self.banner.unwrap_or(defaults.banner),
            prompt: self.prompt.unwrap_or(defaults.prompt),
            page_size: self.page_size.unwrap_or(defaults.page_size),
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
            window: Window {
                width: self.window.width,
                height: self.window.height,
                fullscreen: self.window.fullscreen.unwrap_or(defaults.window.fullscreen),
            },
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
        assert!(!cfg.banner.is_empty());
        assert_eq!(cfg.prompt, "> ");
        assert_eq!(cfg.page_size, 10);
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
        assert_eq!(cfg.banner, "hello");
        assert_eq!(cfg.prompt, "$ ");
        assert_eq!(cfg.page_size, 5);
        assert_eq!(cfg.colors.banner, Color::Red);
        assert_eq!(cfg.colors.prompt, Color::Blue);
        assert_eq!(cfg.colors.selected, Color::Green);
        assert_eq!(cfg.colors.empty, Color::White);
    }

    #[test]
    fn partial_toml_fills_missing_with_defaults() {
        let toml = r#"
prompt = "% "
[colors]
banner = "red"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let defaults = Config::default();
        assert_eq!(cfg.prompt, "% ");
        assert_eq!(cfg.banner, defaults.banner);
        assert_eq!(cfg.page_size, defaults.page_size);
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
        let err = Config::from_toml_str("page_size = 0").unwrap_err();
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
    fn window_defaults_to_fullscreen_when_section_missing() {
        let cfg = Config::from_toml_str("").unwrap();
        assert!(cfg.window.fullscreen);
        assert_eq!(cfg.window.width, None);
        assert_eq!(cfg.window.height, None);
    }

    #[test]
    fn window_explicit_size_is_parsed() {
        let toml = r#"[window]
width = 1280
height = 720
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.window.width, Some(1280));
        assert_eq!(cfg.window.height, Some(720));
        assert!(cfg.window.fullscreen);
    }

    #[test]
    fn window_fullscreen_false_is_honored() {
        let toml = r#"[window]
fullscreen = false
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert!(!cfg.window.fullscreen);
    }

    #[test]
    fn window_unknown_key_rejected() {
        let toml = r#"[window]
resizable = true
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn window_type_mismatch_rejected() {
        let toml = r#"[window]
width = "big"
"#;
        let err = Config::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn load_from_reads_file() {
        let dir = TempDir::new();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, r#"prompt = "$ ""#).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.prompt, "$ ");
    }

    #[test]
    fn load_from_surfaces_parse_errors() {
        let dir = TempDir::new();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "banner = ").unwrap();
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
}
