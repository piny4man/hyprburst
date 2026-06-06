use std::fs;
use std::path::PathBuf;

pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub exec: String,
}

impl DesktopEntry {
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

        Some(DesktopEntry {
            id: String::new(),
            name,
            icon,
            exec,
        })
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
    let mut apps = Vec::new();
    let mut dirs = Vec::new();

    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(path).join("applications"));
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    dirs.push(PathBuf::from("/usr/share/applications"));

    for dir in dirs {
        if dir.is_dir()
            && let Ok(entries) = fs::read_dir(&dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "desktop")
                    && let Ok(content) = fs::read_to_string(&path)
                    && let Some(mut app) = DesktopEntry::parse(&content)
                {
                    app.id = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    apps.push(app);
                }
            }
        }
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

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
