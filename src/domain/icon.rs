//! Nerd Font fallback glyphs for desktop entries.
//!
//! The shipped frontends render icons as text glyphs from the launcher's
//! monospace font, so an entry's icon/name only needs to map to the closest
//! [`fallback_glyph`]. (A future image-icon frontend would re-introduce themed
//! file resolution — with sanitization: the previous resolver accepted absolute
//! paths and unsanitized `..` joins from user-writable `.desktop` files.)

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
}
