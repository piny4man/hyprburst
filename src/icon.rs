use std::path::{Path, PathBuf};

const ICON_EXTENSIONS: &[&str] = &["png", "svg", "xpm"];

/// Raster formats Skia's `from_encoded` can decode. The Freya GUI themed-icon
/// path uses only these, so SVG/XPM-only entries resolve to `None` and fall back
/// to a glyph rather than handing Skia bytes it can't decode.
#[allow(dead_code)]
pub const RASTER_ICON_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];

#[allow(dead_code)]
pub fn resolve_icon(name: &str) -> Option<PathBuf> {
    resolve_icon_in(name, &icon_search_paths())
}

/// Resolve `name` to a raster icon file (see [`RASTER_ICON_EXTENSIONS`]) the GUI
/// can decode, searching the system theme paths. Used by the Freya themed-icon
/// path; returns `None` when only vector/unsupported formats exist.
#[allow(dead_code)]
pub fn resolve_raster_icon(name: &str) -> Option<PathBuf> {
    resolve_icon_in_ext(name, &icon_search_paths(), RASTER_ICON_EXTENSIONS)
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
    resolve_icon_in_ext(name, search_paths, ICON_EXTENSIONS)
}

/// [`resolve_icon_in`] parameterized by the accepted file extensions, so the GUI
/// can restrict resolution to raster formats while the default path keeps the
/// full `png`/`svg`/`xpm` set.
#[allow(dead_code)]
fn resolve_icon_in_ext(name: &str, search_paths: &[PathBuf], exts: &[&str]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }

    let as_path = Path::new(name);
    if as_path.is_absolute() && as_path.is_file() {
        return Some(as_path.to_path_buf());
    }

    for base in search_paths {
        for ext in exts {
            let candidate = base.join(format!("{}.{}", name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Some(found) = find_icon_recursive(base, name, 4, exts) {
            return Some(found);
        }
    }
    None
}

#[allow(dead_code)]
fn find_icon_recursive(dir: &Path, name: &str, depth: usize, exts: &[&str]) -> Option<PathBuf> {
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
            && exts.contains(&ext)
        {
            return Some(path);
        }
    }
    for sub in subdirs {
        if let Some(found) = find_icon_recursive(&sub, name, depth - 1, exts) {
            return Some(found);
        }
    }
    None
}

const GENERIC_GLYPH: &str = "\u{f1b2}"; // nf-fa-cube

const KEYWORD_GLYPHS: &[(&str, &str)] = &[
    // Browsers
    ("firefox", "\u{f269}"),     // nf-fa-firefox
    ("chromium", "\u{f268}"),    // nf-fa-chrome
    ("chrome", "\u{f268}"),      // nf-fa-chrome
    ("brave", "\u{f0ac}"),       // nf-fa-globe
    ("edge", "\u{f0ac}"),        // nf-fa-globe
    ("opera", "\u{f26a}"),       // nf-fa-opera
    ("vivaldi", "\u{f0ac}"),     // nf-fa-globe
    ("tor-browser", "\u{f21b}"), // nf-fa-user-secret
    ("browser", "\u{f0ac}"),     // nf-fa-globe
    // Terminals
    ("alacritty", "\u{f120}"), // nf-fa-terminal
    ("kitty", "\u{f120}"),
    ("ghostty", "\u{f120}"),
    ("wezterm", "\u{f120}"),
    ("foot", "\u{f120}"),
    ("terminal", "\u{f120}"),
    ("console", "\u{f120}"),
    ("tty", "\u{f120}"),
    // File managers
    ("nautilus", "\u{f07b}"), // nf-fa-folder
    ("dolphin", "\u{f07b}"),
    ("thunar", "\u{f07b}"),
    ("file-manager", "\u{f07b}"),
    ("files", "\u{f07b}"),
    ("disk", "\u{f0a0}"), // nf-fa-hdd-o
    // Mail
    ("thunderbird", "\u{f0e0}"), // nf-fa-envelope
    ("mail", "\u{f0e0}"),
    ("email", "\u{f0e0}"),
    // Chat
    ("discord", "\u{f086}"), // nf-fa-comments
    ("slack", "\u{f086}"),
    ("telegram", "\u{f086}"),
    ("signal", "\u{f086}"),
    ("whatsapp", "\u{f086}"),
    ("element", "\u{f086}"),
    ("matrix", "\u{f086}"),
    ("chat", "\u{f086}"),
    ("messaging", "\u{f086}"),
    // Media
    ("spotify", "\u{f1bc}"), // nf-fa-spotify
    ("vlc", "\u{f008}"),     // nf-fa-film
    ("mpv", "\u{f008}"),
    ("obs", "\u{f03d}"),   // nf-fa-video-camera
    ("music", "\u{f001}"), // nf-fa-music
    ("audio", "\u{f001}"),
    ("video", "\u{f03d}"),
    ("media", "\u{f008}"),
    // Images / creative
    ("gimp", "\u{f1fc}"), // nf-fa-paint-brush
    ("inkscape", "\u{f1fc}"),
    ("krita", "\u{f1fc}"),
    ("blender", "\u{f1b2}"), // nf-fa-cube
    ("photo", "\u{f03e}"),   // nf-fa-picture-o
    ("image", "\u{f03e}"),
    ("camera", "\u{f030}"), // nf-fa-camera
    ("screenshot", "\u{f030}"),
    // Editors / dev
    ("vscode", "\u{f121}"), // nf-fa-code
    ("code", "\u{f121}"),
    ("neovim", "\u{f044}"), // nf-fa-edit
    ("vim", "\u{f044}"),
    ("emacs", "\u{f044}"),
    ("sublime", "\u{f044}"),
    ("editor", "\u{f044}"),
    ("ide", "\u{f121}"),
    ("developer", "\u{f109}"), // nf-fa-laptop
    // Settings
    ("preferences", "\u{f013}"), // nf-fa-cog
    ("configuration", "\u{f013}"),
    ("control-center", "\u{f013}"),
    ("settings", "\u{f013}"),
    // Office
    ("libreoffice", "\u{f0f6}"), // nf-fa-file-text-o
    ("writer", "\u{f0f6}"),
    ("impress", "\u{f080}"), // nf-fa-bar-chart
    ("calc", "\u{f080}"),
    ("office", "\u{f0f6}"),
    ("document", "\u{f0f6}"),
    ("pdf", "\u{f1c1}"), // nf-fa-file-pdf-o
    // Utilities
    ("calculator", "\u{f1ec}"), // nf-fa-calculator
    ("characters", "\u{f031}"), // nf-fa-font
    ("clock", "\u{f017}"),      // nf-fa-clock-o
    ("calendar", "\u{f073}"),   // nf-fa-calendar
    ("weather", "\u{f0c2}"),    // nf-fa-cloud
    ("news", "\u{f1ea}"),       // nf-fa-newspaper-o
    // Games
    ("steam", "\u{f1b6}"),  // nf-fa-steam
    ("lutris", "\u{f11b}"), // nf-fa-gamepad
    ("game", "\u{f11b}"),
    // Security
    ("1password", "\u{f084}"), // nf-fa-key
    ("bitwarden", "\u{f084}"),
    ("keepassxc", "\u{f084}"),
    ("keepass", "\u{f084}"),
    ("password", "\u{f084}"),
    ("authenticator", "\u{f132}"), // nf-fa-shield
    ("ente-auth", "\u{f132}"),
    ("ente", "\u{f132}"),
    // Network / connectivity
    ("vpn", "\u{f023}"),       // nf-fa-lock
    ("ssh", "\u{f0c1}"),       // nf-fa-link
    ("bluetooth", "\u{f293}"), // nf-fa-bluetooth
    ("network", "\u{f1eb}"),   // nf-fa-wifi
    ("anydesk", "\u{f108}"),   // nf-fa-desktop
    ("remote", "\u{f108}"),
    // System
    ("help", "\u{f059}"),    // nf-fa-question-circle
    ("about", "\u{f05a}"),   // nf-fa-info-circle
    ("printer", "\u{f02f}"), // nf-fa-print
    ("scanner", "\u{f02f}"),
    // Extensions / plugins
    ("extensions", "\u{f12e}"), // nf-fa-puzzle-piece
    ("extension", "\u{f12e}"),
    // DB / data
    ("beekeeper", "\u{f1c0}"), // nf-fa-database
    ("database", "\u{f1c0}"),
    ("sql", "\u{f1c0}"),
    // Misc tools
    ("bruno", "\u{f1e6}"),  // nf-fa-plug
    ("claude", "\u{f121}"), // nf-fa-code
    ("anthropic", "\u{f121}"),
    ("ai", "\u{f121}"),
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

    fn make_icon_dir() -> tempdir_like::Dir {
        tempdir_like::Dir::new("hyprburst-icon-tests")
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
        assert_eq!(fallback_glyph("firefox", "Firefox"), "\u{f269}");
    }

    #[test]
    fn fallback_glyph_matches_app_name_when_icon_empty() {
        assert_eq!(fallback_glyph("", "Terminal"), "\u{f120}");
    }

    #[test]
    fn fallback_glyph_is_case_insensitive() {
        assert_eq!(fallback_glyph("FIREFOX", ""), "\u{f269}");
    }

    #[test]
    fn fallback_glyph_unknown_returns_generic_package() {
        assert_eq!(
            fallback_glyph("unknown-thing", "Mystery App"),
            GENERIC_GLYPH
        );
    }

    #[test]
    fn fallback_glyph_empty_inputs_return_generic() {
        assert_eq!(fallback_glyph("", ""), GENERIC_GLYPH);
    }

    #[test]
    fn fallback_glyph_matches_substring_keyword() {
        // "gnome-terminal" should match the "terminal" keyword
        assert_eq!(fallback_glyph("gnome-terminal", "Terminal"), "\u{f120}");
    }

    #[test]
    fn fallback_glyph_returns_nerd_font_codepoints() {
        // Every mapped glyph must live in the Nerd Font private-use area
        // (U+E000..=U+F8FF) so it doesn't render as a color emoji.
        for (keyword, glyph) in KEYWORD_GLYPHS {
            for ch in glyph.chars() {
                assert!(
                    (0xE000..=0xF8FF).contains(&(ch as u32)),
                    "glyph for {:?} is not a private-use codepoint: U+{:04X}",
                    keyword,
                    ch as u32
                );
            }
        }
        for ch in GENERIC_GLYPH.chars() {
            assert!((0xE000..=0xF8FF).contains(&(ch as u32)));
        }
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
