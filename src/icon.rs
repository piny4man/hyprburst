use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCapability {
    Kitty,
    Sixel,
    Fallback,
}

pub(crate) struct EnvSnapshot {
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub kitty_window_id: Option<String>,
}

impl EnvSnapshot {
    pub fn current() -> Self {
        Self {
            term: std::env::var("TERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").ok(),
        }
    }
}

pub fn detect_capability() -> TerminalCapability {
    detect_from_env(&EnvSnapshot::current())
}

pub(crate) fn detect_from_env(env: &EnvSnapshot) -> TerminalCapability {
    if env
        .kitty_window_id
        .as_deref()
        .is_some_and(|v| !v.is_empty())
    {
        return TerminalCapability::Kitty;
    }

    if let Some(term) = env.term.as_deref() {
        let t = term.to_lowercase();
        if t.contains("kitty") || t.contains("wezterm") || t.contains("ghostty") {
            return TerminalCapability::Kitty;
        }
        if t.contains("foot") || t.contains("mlterm") {
            return TerminalCapability::Sixel;
        }
    }

    if let Some(prog) = env.term_program.as_deref() {
        let p = prog.to_lowercase();
        if p.contains("kitty") || p.contains("wezterm") || p.contains("ghostty") {
            return TerminalCapability::Kitty;
        }
    }

    TerminalCapability::Fallback
}

const ICON_EXTENSIONS: &[&str] = &["png", "svg", "xpm"];

#[allow(dead_code)]
pub fn resolve_icon(name: &str) -> Option<PathBuf> {
    resolve_icon_in(name, &icon_search_paths())
}

#[allow(dead_code)]
pub(crate) fn icon_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(&home).join(".icons"));
        if std::env::var("XDG_DATA_HOME").is_err() {
            paths.push(PathBuf::from(&home).join(".local/share/icons"));
        }
    }
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        paths.push(PathBuf::from(data_home).join("icons"));
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
            paths.push(PathBuf::from(dir).join("icons"));
        }
    } else {
        paths.push(PathBuf::from("/usr/local/share/icons"));
        paths.push(PathBuf::from("/usr/share/icons"));
    }
    paths.push(PathBuf::from("/usr/share/pixmaps"));
    paths
}

#[allow(dead_code)]
pub(crate) fn resolve_icon_in(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }

    let as_path = Path::new(name);
    if as_path.is_absolute() && as_path.is_file() {
        return Some(as_path.to_path_buf());
    }

    for base in search_paths {
        for ext in ICON_EXTENSIONS {
            let candidate = base.join(format!("{}.{}", name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Some(found) = find_icon_recursive(base, name, 4) {
            return Some(found);
        }
    }
    None
}

fn find_icon_recursive(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !dir.is_dir() {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem == name
            && let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ICON_EXTENSIONS.contains(&ext)
        {
            return Some(path);
        }
    }
    for sub in subdirs {
        if let Some(found) = find_icon_recursive(&sub, name, depth - 1) {
            return Some(found);
        }
    }
    None
}

const GENERIC_GLYPH: &str = "📦";

const KEYWORD_GLYPHS: &[(&str, &str)] = &[
    // Browsers
    ("firefox", "🦊"),
    ("chromium", "🌐"),
    ("chrome", "🌐"),
    ("brave", "🦁"),
    ("edge", "🌐"),
    ("opera", "🎭"),
    ("vivaldi", "🌐"),
    ("tor-browser", "🧅"),
    ("browser", "🌐"),
    // Terminals
    ("alacritty", "⌨️"),
    ("kitty", "⌨️"),
    ("ghostty", "👻"),
    ("wezterm", "⌨️"),
    ("foot", "⌨️"),
    ("terminal", "⌨️"),
    ("console", "⌨️"),
    ("tty", "⌨️"),
    // File managers
    ("nautilus", "📁"),
    ("dolphin", "📁"),
    ("thunar", "📁"),
    ("file-manager", "📁"),
    ("files", "📁"),
    ("disk", "💽"),
    // Mail
    ("thunderbird", "📧"),
    ("mail", "📧"),
    ("email", "📧"),
    // Chat
    ("discord", "💬"),
    ("slack", "💬"),
    ("telegram", "💬"),
    ("signal", "💬"),
    ("whatsapp", "💬"),
    ("element", "💬"),
    ("matrix", "💬"),
    ("chat", "💬"),
    ("messaging", "💬"),
    // Media
    ("spotify", "🎵"),
    ("vlc", "🎬"),
    ("mpv", "🎬"),
    ("obs", "🎥"),
    ("music", "🎵"),
    ("audio", "🎵"),
    ("video", "🎥"),
    ("media", "🎬"),
    // Images / creative
    ("gimp", "🎨"),
    ("inkscape", "🎨"),
    ("krita", "🎨"),
    ("blender", "🧊"),
    ("photo", "🖼️"),
    ("image", "🖼️"),
    ("camera", "📷"),
    ("screenshot", "📸"),
    // Editors / dev
    ("vscode", "📝"),
    ("code", "📝"),
    ("neovim", "📝"),
    ("vim", "📝"),
    ("emacs", "📝"),
    ("sublime", "📝"),
    ("editor", "📝"),
    ("ide", "💻"),
    ("developer", "💻"),
    // Settings
    ("preferences", "⚙️"),
    ("configuration", "⚙️"),
    ("control-center", "⚙️"),
    ("settings", "⚙️"),
    // Office
    ("libreoffice", "📄"),
    ("writer", "📄"),
    ("impress", "📊"),
    ("calc", "📊"),
    ("office", "📄"),
    ("document", "📄"),
    ("pdf", "📄"),
    // Utilities
    ("calculator", "🧮"),
    ("characters", "🔤"),
    ("clock", "🕐"),
    ("calendar", "📅"),
    ("weather", "☁️"),
    ("news", "📰"),
    // Games
    ("steam", "🎮"),
    ("lutris", "🎮"),
    ("game", "🎮"),
    // Security
    ("1password", "🔑"),
    ("bitwarden", "🔑"),
    ("keepassxc", "🔑"),
    ("keepass", "🔑"),
    ("password", "🔑"),
    ("authenticator", "🔐"),
    ("ente-auth", "🔐"),
    ("ente", "🔐"),
    // Network / connectivity
    ("vpn", "🔒"),
    ("ssh", "🔗"),
    ("bluetooth", "🔵"),
    ("network", "🌐"),
    ("anydesk", "🖥️"),
    ("remote", "🖥️"),
    // System
    ("help", "❓"),
    ("about", "ℹ️"),
    ("printer", "🖨️"),
    ("scanner", "🖨️"),
    // Extensions / plugins
    ("extensions", "🧩"),
    ("extension", "🧩"),
    // DB / data
    ("beekeeper", "🐝"),
    ("database", "🗄️"),
    ("sql", "🗄️"),
    // Misc tools
    ("bruno", "🥖"),
    ("claude", "🤖"),
    ("anthropic", "🤖"),
    ("ai", "🤖"),
];

pub fn fallback_glyph(icon_name: &str, app_name: &str) -> &'static str {
    let haystack = format!("{} {}", icon_name, app_name).to_lowercase();
    for (kw, glyph) in KEYWORD_GLYPHS {
        if haystack.contains(kw) {
            return glyph;
        }
    }
    GENERIC_GLYPH
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn env(term: Option<&str>, prog: Option<&str>, kitty: Option<&str>) -> EnvSnapshot {
        EnvSnapshot {
            term: term.map(String::from),
            term_program: prog.map(String::from),
            kitty_window_id: kitty.map(String::from),
        }
    }

    #[test]
    fn detect_kitty_via_window_id() {
        let e = env(Some("xterm-256color"), None, Some("1"));
        assert_eq!(detect_from_env(&e), TerminalCapability::Kitty);
    }

    #[test]
    fn detect_kitty_via_term() {
        let e = env(Some("xterm-kitty"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Kitty);
    }

    #[test]
    fn detect_kitty_via_wezterm() {
        let e = env(Some("wezterm"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Kitty);
    }

    #[test]
    fn detect_kitty_via_ghostty() {
        let e = env(Some("xterm-ghostty"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Kitty);
    }

    #[test]
    fn detect_sixel_via_foot() {
        let e = env(Some("foot"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Sixel);
    }

    #[test]
    fn detect_sixel_via_mlterm() {
        let e = env(Some("mlterm"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Sixel);
    }

    #[test]
    fn detect_fallback_on_plain_xterm() {
        let e = env(Some("xterm-256color"), None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Fallback);
    }

    #[test]
    fn detect_fallback_when_env_missing() {
        let e = env(None, None, None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Fallback);
    }

    #[test]
    fn detect_kitty_via_term_program() {
        let e = env(Some("xterm-256color"), Some("WezTerm"), None);
        assert_eq!(detect_from_env(&e), TerminalCapability::Kitty);
    }

    #[test]
    fn empty_kitty_window_id_is_not_kitty() {
        let e = env(Some("xterm-256color"), None, Some(""));
        assert_eq!(detect_from_env(&e), TerminalCapability::Fallback);
    }

    fn make_icon_dir() -> tempdir_like::Dir {
        tempdir_like::Dir::new("burst-icon-tests")
    }

    #[test]
    fn resolve_returns_none_for_empty_name() {
        assert!(resolve_icon_in("", &[]).is_none());
    }

    #[test]
    fn resolve_returns_none_for_missing_icon() {
        let dir = make_icon_dir();
        assert!(resolve_icon_in("nope", &[dir.path().to_path_buf()]).is_none());
    }

    #[test]
    fn resolve_absolute_path_when_file_exists() {
        let dir = make_icon_dir();
        let file = dir.path().join("abs.png");
        fs::write(&file, b"x").unwrap();
        let got = resolve_icon_in(file.to_str().unwrap(), &[]).unwrap();
        assert_eq!(got, file);
    }

    #[test]
    fn resolve_absolute_path_none_when_missing() {
        let dir = make_icon_dir();
        let file = dir.path().join("missing.png");
        assert!(resolve_icon_in(file.to_str().unwrap(), &[]).is_none());
    }

    #[test]
    fn resolve_finds_icon_direct_in_search_path() {
        let dir = make_icon_dir();
        let file = dir.path().join("firefox.png");
        fs::write(&file, b"x").unwrap();
        let got = resolve_icon_in("firefox", &[dir.path().to_path_buf()]).unwrap();
        assert_eq!(got, file);
    }

    #[test]
    fn resolve_finds_icon_in_nested_theme_dir() {
        let dir = make_icon_dir();
        let nested = dir.path().join("hicolor").join("48x48").join("apps");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("firefox.svg");
        fs::write(&file, b"x").unwrap();
        let got = resolve_icon_in("firefox", &[dir.path().to_path_buf()]).unwrap();
        assert_eq!(got, file);
    }

    #[test]
    fn resolve_prefers_direct_match_over_nested() {
        let dir = make_icon_dir();
        let nested = dir.path().join("hicolor").join("48x48").join("apps");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("firefox.png"), b"nested").unwrap();
        let direct = dir.path().join("firefox.png");
        fs::write(&direct, b"direct").unwrap();
        let got = resolve_icon_in("firefox", &[dir.path().to_path_buf()]).unwrap();
        assert_eq!(got, direct);
    }

    #[test]
    fn resolve_ignores_unsupported_extensions() {
        let dir = make_icon_dir();
        fs::write(dir.path().join("firefox.bmp"), b"x").unwrap();
        assert!(resolve_icon_in("firefox", &[dir.path().to_path_buf()]).is_none());
    }

    #[test]
    fn fallback_glyph_matches_known_icon_name() {
        assert_eq!(fallback_glyph("firefox", "Firefox"), "🦊");
    }

    #[test]
    fn fallback_glyph_matches_app_name_when_icon_empty() {
        assert_eq!(fallback_glyph("", "Terminal"), "⌨\u{fe0f}");
    }

    #[test]
    fn fallback_glyph_is_case_insensitive() {
        assert_eq!(fallback_glyph("FIREFOX", ""), "🦊");
    }

    #[test]
    fn fallback_glyph_unknown_returns_generic_package() {
        assert_eq!(fallback_glyph("unknown-thing", "Mystery App"), "📦");
    }

    #[test]
    fn fallback_glyph_empty_inputs_return_generic() {
        assert_eq!(fallback_glyph("", ""), "📦");
    }

    #[test]
    fn fallback_glyph_matches_substring_keyword() {
        // "gnome-terminal" should match the "terminal" keyword
        assert_eq!(fallback_glyph("gnome-terminal", "Terminal"), "⌨\u{fe0f}");
    }

    mod tempdir_like {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        pub struct Dir {
            path: PathBuf,
        }

        impl Dir {
            pub fn new(prefix: &str) -> Self {
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let pid = std::process::id();
                let path = std::env::temp_dir().join(format!("{}-{}-{}", prefix, pid, n));
                std::fs::create_dir_all(&path).unwrap();
                Self { path }
            }

            pub fn path(&self) -> &Path {
                &self.path
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
}
