use std::fs;
use std::path::PathBuf;

use crate::domain::icon::fallback_glyph;

pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    /// Lowercased [`Self::name`], precomputed once at discovery so the search
    /// match and sort tie-break never allocate per comparison or per keystroke.
    pub(crate) name_lower: String,
    pub icon: String,
    pub exec: String,
    /// Nerd Font fallback glyph for this entry (see
    /// [`fallback_glyph`](crate::domain::icon::fallback_glyph)), resolved once
    /// at discovery instead of per visible cell per frame.
    pub(crate) glyph: &'static str,
}

impl DesktopEntry {
    /// Build an entry, deriving the lowercase search name and fallback glyph
    /// from the immutable fields. The single derivation point shared by
    /// discovery and tests.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        icon: impl Into<String>,
        exec: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let name_lower = name.to_lowercase();
        let icon = icon.into();
        let glyph = fallback_glyph(&icon, &name);
        Self {
            id: id.into(),
            name,
            name_lower,
            icon,
            exec: exec.into(),
            glyph,
        }
    }

    fn parse(content: &str) -> Option<Self> {
        let mut name = None;
        let mut icon = None;
        let mut exec = None;
        let mut hidden = false;
        let mut no_display = false;
        let mut in_desktop_entry = false;

        for line in content.lines() {
            let line = line.trim();

            if line == "[Desktop Entry]" {
                in_desktop_entry = true;
                continue;
            }

            if line.starts_with('[') {
                in_desktop_entry = false;
                continue;
            }

            if !in_desktop_entry {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Name" if name.is_none() => name = Some(value.to_string()),
                    "Icon" => icon = Some(value.to_string()),
                    "Exec" => exec = Some(value.to_string()),
                    "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
                    "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                    _ => {}
                }
            }
        }

        let name = name?;
        if hidden || no_display {
            return None;
        }

        let icon = icon.unwrap_or_default();
        let exec = exec
            .map(|exec| expand_exec_field_codes(&exec, &name, &icon))
            .unwrap_or_default();

        Some(Self::new(String::new(), name, icon, exec))
    }
}

fn expand_exec_field_codes(exec: &str, name: &str, icon: &str) -> String {
    let mut out = String::with_capacity(exec.len());

    for token in exec_tokens(exec) {
        if token_has_target_field_code(&token) {
            continue;
        }

        let expanded = expand_non_target_field_codes(&token, name, icon);
        if !expanded.is_empty() {
            push_spaced(&mut out, &expanded);
        }
    }

    out
}

fn exec_tokens(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in exec.trim().chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                token.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                token.push(c);
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(c),
        }
    }

    if !token.is_empty() {
        tokens.push(token);
    }

    tokens
}

fn token_has_target_field_code(token: &str) -> bool {
    let mut chars = token.chars();

    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }

        match chars.next() {
            Some('%') => {}
            Some('f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => return true,
            Some(_) | None => {}
        }
    }

    false
}

fn expand_non_target_field_codes(exec: &str, name: &str, icon: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars();

    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }

        match chars.next() {
            Some('%') => out.push('%'),
            Some('i') if !icon.is_empty() => {
                push_spaced(&mut out, "--icon");
                push_spaced(&mut out, &shell_quote_arg(icon));
            }
            Some('i' | 'k') => {}
            Some('c') => push_spaced(&mut out, &shell_quote_arg(name)),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }

    out
}

fn shell_quote_arg(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'))
    {
        return value.to_string();
    }

    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn push_spaced(out: &mut String, value: &str) {
    if !out.is_empty() && !out.ends_with(char::is_whitespace) {
        out.push(' ');
    }
    out.push_str(value);
}

pub fn discover_apps() -> Vec<DesktopEntry> {
    discover_in(&data_dirs())
}

/// Data roots per the XDG basedir spec, in precedence order: `$XDG_DATA_HOME`
/// (default `~/.local/share`) first so user entries shadow system ones, then
/// each colon-separated entry of `$XDG_DATA_DIRS` (default
/// `/usr/local/share:/usr/share`). Honoring `XDG_DATA_DIRS` is also what makes
/// Flatpak-style export dirs (`…/exports/share`) visible.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = std::env::var("XDG_DATA_HOME")
        && !path.is_empty()
    {
        dirs.push(PathBuf::from(path));
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let data_dirs = match std::env::var("XDG_DATA_DIRS") {
        Ok(v) if !v.is_empty() => v,
        _ => "/usr/local/share:/usr/share".to_string(),
    };
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    dirs
}

/// Discover `.desktop` applications under each data root's `applications/`
/// subdir. The *first* file for a given desktop-id wins — per the spec a
/// higher-precedence copy shadows later ones, and that holds even when the
/// shadowing file is `Hidden`/`NoDisplay` (a user can hide a system app by
/// overriding it) or fails to parse.
fn discover_in(data_roots: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut apps = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in data_roots {
        let dir = root.join("applications");
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "desktop") {
                continue;
            }
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path)
                && let Some(mut app) = DesktopEntry::parse(&content)
            {
                app.id = id;
                apps.push(app);
            }
        }
    }

    apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::testutil::Dir as TempDir;
    use std::path::Path;

    /// Write a minimal `.desktop` entry under `<root>/applications/`.
    fn write_entry(root: &Path, file_name: &str, name: &str) {
        let dir = root.join("applications");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(file_name),
            format!("[Desktop Entry]\nName={name}\nExec=app-{name}\n"),
        )
        .unwrap();
    }

    #[test]
    fn discovery_prefers_first_root_and_dedups_by_id() {
        let user = TempDir::new("hyprburst-disc-user");
        let system = TempDir::new("hyprburst-disc-sys");
        write_entry(user.path(), "firefox.desktop", "Firefox User");
        write_entry(system.path(), "firefox.desktop", "Firefox System");
        write_entry(system.path(), "only-system.desktop", "System Only");

        let apps = discover_in(&[user.path().to_path_buf(), system.path().to_path_buf()]);

        assert_eq!(apps.len(), 2, "the shadowed firefox.desktop is skipped");
        assert_eq!(apps[0].id, "firefox");
        assert_eq!(apps[0].name, "Firefox User", "first root wins");
        assert_eq!(apps[1].id, "only-system");
    }

    #[test]
    fn discovery_reads_custom_xdg_dir_like_flatpak_exports() {
        let exports = TempDir::new("hyprburst-disc-flatpak");
        write_entry(exports.path(), "com.example.App.desktop", "Flatpak App");

        let apps = discover_in(&[exports.path().to_path_buf()]);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "com.example.App");
        assert_eq!(apps[0].name, "Flatpak App");
    }

    #[test]
    fn discovery_hidden_override_shadows_system_entry() {
        // A user's NoDisplay copy of a system id hides it entirely.
        let user = TempDir::new("hyprburst-disc-hide");
        let system = TempDir::new("hyprburst-disc-hide2");
        fs::create_dir_all(user.path().join("applications")).unwrap();
        fs::write(
            user.path().join("applications").join("secret.desktop"),
            "[Desktop Entry]\nName=Secret\nExec=secret\nNoDisplay=true\n",
        )
        .unwrap();
        write_entry(system.path(), "secret.desktop", "Secret System");

        let apps = discover_in(&[user.path().to_path_buf(), system.path().to_path_buf()]);
        assert!(apps.is_empty(), "NoDisplay override hides the system app");
    }

    #[test]
    fn discovery_ignores_roots_without_applications_dir() {
        let empty = TempDir::new("hyprburst-disc-empty");
        let apps = discover_in(&[empty.path().to_path_buf()]);
        assert!(apps.is_empty());
    }

    #[test]
    fn parses_valid_desktop_entry() {
        let content = r#"[Desktop Entry]
Name=Firefox
Icon=firefox
Exec=firefox %u
Type=Application
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.name, "Firefox");
        assert_eq!(entry.icon, "firefox");
        assert_eq!(entry.exec, "firefox");
    }

    #[test]
    fn removes_file_placeholders_from_exec() {
        let content = r#"[Desktop Entry]
Name=Inkscape
Icon=org.inkscape.Inkscape
Exec=inkscape %F
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.exec, "inkscape");
    }

    #[test]
    fn removes_url_placeholders_from_exec() {
        let content = r#"[Desktop Entry]
Name=Browser
Exec=brave %U
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.exec, "brave");
    }

    #[test]
    fn removes_option_token_when_it_contains_url_placeholder() {
        let content = r#"[Desktop Entry]
Name=Spotify
Exec=spotify --uri=%u
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.exec, "spotify");
    }

    #[test]
    fn preserves_quoted_executable_when_removing_placeholders() {
        let content = r#"[Desktop Entry]
Name=Beekeeper Studio
Exec="/opt/Beekeeper Studio/beekeeper-studio" %U
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.exec, "\"/opt/Beekeeper Studio/beekeeper-studio\"");
    }

    #[test]
    fn expands_literal_percent_icon_and_name_placeholders() {
        let content = r#"[Desktop Entry]
Name=Fancy App
Icon=fancy-icon
Exec=fancy --rate %% --label %c %i
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(
            entry.exec,
            "fancy --rate % --label 'Fancy App' --icon fancy-icon"
        );
    }

    #[test]
    fn filters_hidden_entries() {
        let content = r#"[Desktop Entry]
Name=Hidden App
Icon=hidden
Exec=hidden-app
Hidden=true
"#;
        assert!(DesktopEntry::parse(content).is_none());
    }

    #[test]
    fn filters_no_display_entries() {
        let content = r#"[Desktop Entry]
Name=NoDisplay App
Icon=nodisplay
Exec=nodisplay-app
NoDisplay=true
"#;
        assert!(DesktopEntry::parse(content).is_none());
    }

    #[test]
    fn missing_name_returns_none() {
        let content = r#"[Desktop Entry]
Icon=firefox
Exec=firefox %u
"#;
        assert!(DesktopEntry::parse(content).is_none());
    }

    #[test]
    fn missing_icon_defaults_empty() {
        let content = r#"[Desktop Entry]
Name=Test App
Exec=test-app
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.icon, "");
    }

    #[test]
    fn missing_exec_defaults_empty() {
        let content = r#"[Desktop Entry]
Name=Test App
Icon=test-icon
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.exec, "");
    }

    #[test]
    fn only_parses_desktop_entry_section() {
        let content = r#"[Desktop Entry]
Name=Test App
Icon=test
Exec=test-app

[Desktop Action NewWindow]
Name=New Window
Exec=test-app --new-window
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.name, "Test App");
        assert_eq!(entry.exec, "test-app");
    }

    #[test]
    fn first_name_wins_duplicates() {
        let content = r#"[Desktop Entry]
Name=First Name
Name=Second Name
Exec=test-app
"#;
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.name, "First Name");
    }

    #[test]
    fn case_insensitive_hidden_check() {
        let content = r#"[Desktop Entry]
Name=Hidden App
Exec=hidden-app
Hidden=TRUE
"#;
        assert!(DesktopEntry::parse(content).is_none());
    }
}
