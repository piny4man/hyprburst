use std::path::PathBuf;

use crate::config::Config;

pub const DEFAULT_SIZE: f32 = 14.0;

pub const NERD_FONT_PREFERENCE: &[&str] = &[
    "JetBrainsMono Nerd Font",
    "FiraCode Nerd Font",
    "Symbols Nerd Font",
];

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFont {
    pub path: PathBuf,
    pub index: u32,
    pub size: f32,
}

pub trait FontDatabase {
    fn lookup_family(&self, family: &str) -> Option<(PathBuf, u32)>;
    fn first_monospace(&self) -> Option<(PathBuf, u32)>;
}

pub fn resolve(config: &Config, db: &dyn FontDatabase) -> Option<LoadedFont> {
    let size = config.font.size.unwrap_or(DEFAULT_SIZE);

    if let Some(configured) = config.font.path.as_ref() {
        if configured.exists() {
            return Some(LoadedFont {
                path: configured.clone(),
                index: 0,
                size,
            });
        }
        eprintln!(
            "burst: configured font {:?} not found, falling back to system lookup",
            configured
        );
    }

    for family in NERD_FONT_PREFERENCE {
        if let Some((path, index)) = db.lookup_family(family) {
            return Some(LoadedFont { path, index, size });
        }
    }

    db.first_monospace()
        .map(|(path, index)| LoadedFont { path, index, size })
}

pub struct SystemFontDatabase {
    db: fontdb::Database,
}

impl SystemFontDatabase {
    pub fn load() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self { db }
    }
}

impl Default for SystemFontDatabase {
    fn default() -> Self {
        Self::load()
    }
}

impl FontDatabase for SystemFontDatabase {
    fn lookup_family(&self, family: &str) -> Option<(PathBuf, u32)> {
        let id = self.db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..fontdb::Query::default()
        })?;
        let face = self.db.face(id)?;
        source_path(&face.source).map(|p| (p, face.index))
    }

    fn first_monospace(&self) -> Option<(PathBuf, u32)> {
        self.db
            .faces()
            .find(|f| f.monospaced)
            .and_then(|f| source_path(&f.source).map(|p| (p, f.index)))
    }
}

fn source_path(source: &fontdb::Source) -> Option<PathBuf> {
    match source {
        fontdb::Source::File(p) => Some(p.clone()),
        fontdb::Source::SharedFile(p, _) => Some(p.clone()),
        fontdb::Source::Binary(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::config::Font;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("burst-font-test-{}-{}", std::process::id(), n));
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

    #[derive(Default)]
    struct FakeDb {
        families: HashMap<String, (PathBuf, u32)>,
        monospace: Option<(PathBuf, u32)>,
    }

    impl FakeDb {
        fn with_family(mut self, family: &str, path: &str) -> Self {
            self.families
                .insert(family.to_string(), (PathBuf::from(path), 0));
            self
        }

        fn with_monospace(mut self, path: &str) -> Self {
            self.monospace = Some((PathBuf::from(path), 0));
            self
        }
    }

    impl FontDatabase for FakeDb {
        fn lookup_family(&self, family: &str) -> Option<(PathBuf, u32)> {
            self.families.get(family).cloned()
        }

        fn first_monospace(&self) -> Option<(PathBuf, u32)> {
            self.monospace.clone()
        }
    }

    fn config_with_font(font: Font) -> Config {
        Config {
            font,
            ..Config::default()
        }
    }

    #[test]
    fn configured_path_used_when_it_exists() {
        let dir = TempDir::new();
        let font_path = dir.path().join("custom.ttf");
        std::fs::write(&font_path, b"fake-font-bytes").unwrap();

        let cfg = config_with_font(Font {
            path: Some(font_path.clone()),
            size: None,
        });
        let db = FakeDb::default();

        let loaded = resolve(&cfg, &db).expect("font resolved");
        assert_eq!(loaded.path, font_path);
        assert_eq!(loaded.size, DEFAULT_SIZE);
    }

    #[test]
    fn configured_path_missing_falls_back_to_system_lookup() {
        let cfg = config_with_font(Font {
            path: Some(PathBuf::from("/definitely/not/here.ttf")),
            size: None,
        });
        let db =
            FakeDb::default().with_family("JetBrainsMono Nerd Font", "/system/jetbrains-mono.ttf");

        let loaded = resolve(&cfg, &db).expect("fallback resolved");
        assert_eq!(loaded.path, PathBuf::from("/system/jetbrains-mono.ttf"));
    }

    #[test]
    fn no_path_picks_first_nerd_font_in_preference_order() {
        let cfg = config_with_font(Font::default());
        let db = FakeDb::default()
            .with_family("FiraCode Nerd Font", "/system/firacode.ttf")
            .with_family("Symbols Nerd Font", "/system/symbols.ttf");

        let loaded = resolve(&cfg, &db).expect("nerd font resolved");
        assert_eq!(loaded.path, PathBuf::from("/system/firacode.ttf"));
    }

    #[test]
    fn jetbrains_preferred_over_other_nerd_fonts() {
        let cfg = config_with_font(Font::default());
        let db = FakeDb::default()
            .with_family("JetBrainsMono Nerd Font", "/system/jb.ttf")
            .with_family("FiraCode Nerd Font", "/system/fira.ttf")
            .with_family("Symbols Nerd Font", "/system/symbols.ttf");

        let loaded = resolve(&cfg, &db).expect("jetbrains resolved");
        assert_eq!(loaded.path, PathBuf::from("/system/jb.ttf"));
    }

    #[test]
    fn no_nerd_font_falls_back_to_first_monospace() {
        let cfg = config_with_font(Font::default());
        let db = FakeDb::default().with_monospace("/system/dejavu-sans-mono.ttf");

        let loaded = resolve(&cfg, &db).expect("monospace resolved");
        assert_eq!(loaded.path, PathBuf::from("/system/dejavu-sans-mono.ttf"));
    }

    #[test]
    fn nothing_available_returns_none() {
        let cfg = config_with_font(Font::default());
        let db = FakeDb::default();
        assert!(resolve(&cfg, &db).is_none());
    }

    #[test]
    fn default_size_is_14pt() {
        let cfg = config_with_font(Font::default());
        let db = FakeDb::default().with_monospace("/system/mono.ttf");

        let loaded = resolve(&cfg, &db).unwrap();
        assert_eq!(loaded.size, 14.0);
    }

    #[test]
    fn configured_size_overrides_default() {
        let cfg = config_with_font(Font {
            path: None,
            size: Some(18.5),
        });
        let db = FakeDb::default().with_monospace("/system/mono.ttf");

        let loaded = resolve(&cfg, &db).unwrap();
        assert_eq!(loaded.size, 18.5);
    }

    #[test]
    fn configured_size_applies_to_configured_path() {
        let dir = TempDir::new();
        let font_path = dir.path().join("custom.ttf");
        std::fs::write(&font_path, b"x").unwrap();

        let cfg = config_with_font(Font {
            path: Some(font_path.clone()),
            size: Some(20.0),
        });

        let db = FakeDb::default();
        let loaded = resolve(&cfg, &db).unwrap();
        assert_eq!(loaded.path, font_path);
        assert_eq!(loaded.size, 20.0);
    }
}
