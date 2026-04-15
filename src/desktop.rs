use std::fs;
use std::path::PathBuf;

pub struct DesktopEntry {
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

        Some(DesktopEntry {
            name,
            icon: icon.unwrap_or_default(),
            exec: exec.unwrap_or_default(),
        })
    }
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
                    && let Some(app) = DesktopEntry::parse(&content)
                {
                    apps.push(app);
                }
            }
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
        assert_eq!(entry.exec, "firefox %u");
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
