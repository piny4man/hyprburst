//! Frontend-agnostic launcher core: the state machine and the data it drives on.
//!
//! Nothing here renders. [`launcher_core`] owns the state machine and abstract
//! [`LauncherAction`](launcher_core::LauncherAction) vocabulary; the rest are the
//! pure utilities it composes — [`config`] (settings schema), [`desktop`] (`.desktop`
//! discovery), [`search`] (ranking), [`history`] (SQLite persistence), and [`icon`]
//! (glyph/theme resolution).

pub mod config;
pub mod desktop;
pub mod history;
pub mod icon;
pub mod launcher_core;
pub mod search;

#[cfg(test)]
pub(crate) mod testutil {
    //! Shared test helpers (self-cleaning temp dirs).

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
