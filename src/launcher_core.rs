//! Frontend-agnostic launcher state machine.
//!
//! `LauncherCore` owns the apps, query, filtered results, selection, history,
//! scores, config, and grid column count. Frontends drive it exclusively
//! through the abstract [`LauncherAction`] vocabulary and render from the
//! read-only [`LauncherView`] projection — no `ratatui`, `crossterm`, or
//! `KeyCode` types appear in this module's public API.

use std::collections::HashMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::desktop::{DesktopEntry, discover_apps};
use crate::history::{History, score as history_score};
use crate::icon::fallback_glyph;
use crate::search::filter_and_rank;

/// Abstract interaction vocabulary for the launcher.
///
/// Frontends translate their native input (crossterm `KeyCode`, Freya key/mouse
/// events, …) into these actions; the core never sees terminal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PageUp,
    PageDown,
    Backspace,
    Insert(char),
    Autocomplete,
    LaunchSelected,
    Cancel,
}

/// Why the visible list is empty, so frontends can pick the right message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReason {
    /// No apps were discovered at all (the query is empty).
    NoApps,
    /// Apps exist but none match the current query.
    NoMatches,
}

/// A single visible entry as the frontend should render it.
pub struct EntryView<'a> {
    pub name: &'a str,
    pub icon_glyph: &'static str,
    pub selected: bool,
}

/// Read-only projection of the core's state for rendering.
pub struct LauncherView<'a> {
    pub query: &'a str,
    pub columns: u16,
    /// `Some(reason)` when there are no entries to show, else `None`.
    pub empty_reason: Option<EmptyReason>,
    pub entries: Vec<EntryView<'a>>,
}

/// The launcher state machine, shared across every frontend variant.
pub struct LauncherCore {
    apps: Vec<DesktopEntry>,
    query: String,
    filtered: Vec<(usize, u32)>,
    selected_index: usize,
    running: bool,
    history: Option<History>,
    scores: HashMap<String, f64>,
    config: Config,
    columns: u16,
}

impl LauncherCore {
    /// Construct from config: discover apps, load history/scores, and build the
    /// initial filtered list.
    pub fn new(config: Config) -> Self {
        let apps = discover_apps();
        let running = !apps.is_empty();
        let history = History::open().ok();
        let scores = history.as_ref().map(load_scores).unwrap_or_default();
        let mut core = Self {
            apps,
            query: String::new(),
            filtered: Vec::new(),
            selected_index: 0,
            running,
            history,
            scores,
            config,
            columns: 1,
        };
        core.rebuild_filtered();
        core
    }

    /// Construct from an explicit app set, skipping live discovery and history
    /// I/O. Used by the benchmark harness and the spike POCs so runs are
    /// deterministic and comparable across machines. Starts running when `apps`
    /// is non-empty, mirroring [`new`](Self::new).
    pub fn from_apps(apps: Vec<DesktopEntry>, config: Config) -> Self {
        let running = !apps.is_empty();
        let mut core = Self {
            apps,
            query: String::new(),
            filtered: Vec::new(),
            selected_index: 0,
            running,
            history: None,
            scores: HashMap::new(),
            config,
            columns: 1,
        };
        core.rebuild_filtered();
        core
    }

    /// Whether the launcher is still accepting input.
    pub fn running(&self) -> bool {
        self.running
    }

    /// The config the core was built with (colors, banner, prompt, …).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Set the grid column count used by vertical/horizontal navigation. The
    /// frontend computes this from its layout and pushes it in before rendering.
    pub fn set_columns(&mut self, columns: u16) {
        self.columns = columns.max(1);
    }

    /// Apply an abstract action to the state machine. No-op once the launcher
    /// has stopped running.
    pub fn apply(&mut self, action: LauncherAction) {
        if !self.running {
            return;
        }

        match action {
            LauncherAction::Cancel => self.running = false,
            LauncherAction::Autocomplete => self.autocomplete(),
            LauncherAction::PageUp => self.page_up(),
            LauncherAction::PageDown => self.page_down(),
            LauncherAction::MoveUp => self.move_vertical(-1),
            LauncherAction::MoveDown => self.move_vertical(1),
            LauncherAction::MoveLeft => self.move_horizontal(-1),
            LauncherAction::MoveRight => self.move_horizontal(1),
            LauncherAction::LaunchSelected => self.launch_selected(),
            LauncherAction::Backspace => {
                self.query.pop();
                self.rebuild_filtered();
            }
            LauncherAction::Insert(c) => {
                self.query.push(c);
                self.rebuild_filtered();
            }
        }
    }

    /// Build a read-only view of the current state for rendering.
    pub fn view(&self) -> LauncherView<'_> {
        let empty_reason = if self.filtered.is_empty() {
            Some(if self.query.is_empty() {
                EmptyReason::NoApps
            } else {
                EmptyReason::NoMatches
            })
        } else {
            None
        };
        let entries = self
            .filtered
            .iter()
            .enumerate()
            .map(|(i, &(idx, _score))| {
                let app = &self.apps[idx];
                EntryView {
                    name: &app.name,
                    icon_glyph: icon_glyph_for(app),
                    selected: i == self.selected_index,
                }
            })
            .collect();
        LauncherView {
            query: &self.query,
            columns: self.columns,
            empty_reason,
            entries,
        }
    }

    fn rebuild_filtered(&mut self) {
        let ranked = filter_and_rank(&self.query, &self.apps, &self.scores);
        self.filtered = ranked
            .into_iter()
            .map(|(app, score)| {
                let idx = self.apps.iter().position(|a| std::ptr::eq(a, app)).unwrap();
                (idx, score)
            })
            .collect();
        self.selected_index = 0;
    }

    fn autocomplete(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let (idx, _) = self.filtered[self.selected_index.min(self.filtered.len() - 1)];
        let name = &self.apps[idx].name;
        self.query = name.to_string();
        self.rebuild_filtered();
    }

    fn move_vertical(&mut self, dr: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let cols = self.columns.max(1) as i32;
        let n = self.filtered.len() as i32;
        let idx = self.selected_index as i32;
        let col = idx % cols;
        let rows_in_col = (n - 1 - col).div_euclid(cols) + 1;
        let row_in_col = idx.div_euclid(cols);
        let new_row = (row_in_col + dr).rem_euclid(rows_in_col);
        self.selected_index = (new_row * cols + col) as usize;
    }

    fn move_horizontal(&mut self, dc: i32) {
        if self.filtered.is_empty() || self.columns <= 1 {
            return;
        }
        let cols = self.columns as i32;
        let n = self.filtered.len() as i32;
        let idx = self.selected_index as i32;
        let row = idx.div_euclid(cols);
        let col = idx % cols;
        let last_col_in_row = (n - 1 - row * cols).min(cols - 1);
        let new_col = (col + dc).rem_euclid(last_col_in_row + 1);
        self.selected_index = (row * cols + new_col) as usize;
    }

    fn page_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(self.config.ui.page_size);
    }

    fn page_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected_index =
            (self.selected_index + self.config.ui.page_size).min(self.filtered.len() - 1);
    }

    fn launch_selected(&mut self) {
        if self.selected_index < self.filtered.len() {
            let (idx, _) = self.filtered[self.selected_index];
            let app = &self.apps[idx];
            if !app.exec.is_empty() {
                let _ = Command::new("hyprctl")
                    .args(["dispatch", "exec", "--", &app.exec])
                    .spawn();
            }
            if let Some(history) = &self.history {
                let _ = history.record_launch(&app.id, &app.name);
            }
        }
        self.running = false;
    }
}

/// Resolve the Nerd Font glyph the given app should render with.
pub(crate) fn icon_glyph_for(app: &DesktopEntry) -> &'static str {
    fallback_glyph(&app.icon, &app.name)
}

fn load_scores(history: &History) -> HashMap<String, f64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    history
        .all()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.desktop_id.clone(), history_score(&entry, now)))
        .collect()
}

#[cfg(test)]
impl LauncherCore {
    /// Test-only constructor with explicit apps and config — skips discovery
    /// and history I/O so frontends and the core can be exercised in isolation.
    pub(crate) fn for_test(apps: Vec<DesktopEntry>, config: Config) -> Self {
        let filtered = (0..apps.len()).map(|i| (i, 0u32)).collect();
        Self {
            apps,
            query: String::new(),
            filtered,
            selected_index: 0,
            running: true,
            history: None,
            scores: HashMap::new(),
            config,
            columns: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_apps() -> Vec<DesktopEntry> {
        vec![
            DesktopEntry {
                id: "app-a".into(),
                name: "App A".into(),
                icon: "icon-a".into(),
                exec: "app-a".into(),
            },
            DesktopEntry {
                id: "app-b".into(),
                name: "App B".into(),
                icon: "icon-b".into(),
                exec: "app-b".into(),
            },
            DesktopEntry {
                id: "app-c".into(),
                name: "App C".into(),
                icon: "icon-c".into(),
                exec: "app-c".into(),
            },
        ]
    }

    fn core_with_apps() -> LauncherCore {
        LauncherCore::for_test(three_apps(), Config::default())
    }

    fn core_with_n_apps(n: usize) -> LauncherCore {
        let apps: Vec<DesktopEntry> = (0..n)
            .map(|i| DesktopEntry {
                id: format!("app-{}", i),
                name: format!("App {}", i),
                icon: format!("icon-{}", i),
                exec: format!("app-{}", i),
            })
            .collect();
        LauncherCore::for_test(apps, Config::default())
    }

    #[test]
    fn move_down_moves_selection_down() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::MoveDown);
        assert_eq!(core.selected_index, 1);
    }

    #[test]
    fn move_up_moves_selection_up() {
        let mut core = core_with_apps();
        core.selected_index = 1;
        core.apply(LauncherAction::MoveUp);
        assert_eq!(core.selected_index, 0);
    }

    #[test]
    fn move_down_wraps_to_first() {
        let mut core = core_with_apps();
        core.selected_index = 2;
        core.apply(LauncherAction::MoveDown);
        assert_eq!(core.selected_index, 0);
    }

    #[test]
    fn move_up_wraps_to_last() {
        let mut core = core_with_apps();
        core.selected_index = 0;
        core.apply(LauncherAction::MoveUp);
        assert_eq!(core.selected_index, 2);
    }

    #[test]
    fn launch_selected_stops_core() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::LaunchSelected);
        assert!(!core.running);
    }

    #[test]
    fn cancel_stops_core() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::Cancel);
        assert!(!core.running);
    }

    #[test]
    fn insert_filters_results() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::Insert('a'));
        assert_eq!(core.query, "a");
        assert!(!core.filtered.is_empty());
    }

    #[test]
    fn backspace_removes_char() {
        let mut core = core_with_apps();
        core.query = "a".into();
        core.apply(LauncherAction::Backspace);
        assert_eq!(core.query, "");
    }

    #[test]
    fn autocomplete_fills_top_match() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::Insert('a'));
        core.apply(LauncherAction::Autocomplete);
        assert!(!core.query.is_empty());
    }

    #[test]
    fn page_down_moves_selection_down() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::PageDown);
        assert_eq!(core.selected_index, 10.min(core.filtered.len() - 1));
    }

    #[test]
    fn page_up_moves_selection_up() {
        let mut core = core_with_apps();
        core.selected_index = 15;
        core.apply(LauncherAction::PageUp);
        assert_eq!(core.selected_index, 5);
    }

    #[test]
    fn empty_apps_list_stops_on_cancel() {
        let mut core = LauncherCore::for_test(vec![], Config::default());
        core.apply(LauncherAction::Cancel);
        assert!(!core.running);
    }

    #[test]
    fn insert_resets_selection() {
        let mut core = core_with_apps();
        core.selected_index = 2;
        core.apply(LauncherAction::Insert('A'));
        assert_eq!(core.query, "A");
        assert_eq!(core.selected_index, 0);
    }

    #[test]
    fn launch_selected_records_launch_in_history() {
        let history = History::in_memory().unwrap();
        let mut core = core_with_apps();
        core.history = Some(history);

        core.apply(LauncherAction::LaunchSelected);

        let entry = core
            .history
            .as_ref()
            .unwrap()
            .get("app-a")
            .unwrap()
            .unwrap();
        assert_eq!(entry.launch_count, 1);
        assert_eq!(entry.app_name, "App A");
    }

    #[test]
    fn icon_glyph_maps_known_app_to_nerd_font() {
        let app = DesktopEntry {
            id: "firefox".into(),
            name: "Firefox".into(),
            icon: "firefox".into(),
            exec: "firefox".into(),
        };
        assert_eq!(icon_glyph_for(&app), "\u{f269}");
    }

    #[test]
    fn icon_glyph_unknown_app_returns_generic() {
        let app = DesktopEntry {
            id: "xyz".into(),
            name: "Qwerty".into(),
            icon: "zzz".into(),
            exec: "qwerty".into(),
        };
        assert_eq!(icon_glyph_for(&app), "\u{f1b2}");
    }

    #[test]
    fn grid_down_moves_to_next_row_same_column() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 1;
        core.apply(LauncherAction::MoveDown);
        assert_eq!(core.selected_index, 5);
    }

    #[test]
    fn grid_up_moves_to_prev_row_same_column() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 6;
        core.apply(LauncherAction::MoveUp);
        assert_eq!(core.selected_index, 2);
    }

    #[test]
    fn grid_right_moves_within_row() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 1;
        core.apply(LauncherAction::MoveRight);
        assert_eq!(core.selected_index, 2);
    }

    #[test]
    fn grid_left_moves_within_row() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 2;
        core.apply(LauncherAction::MoveLeft);
        assert_eq!(core.selected_index, 1);
    }

    #[test]
    fn grid_right_wraps_within_row() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 3;
        core.apply(LauncherAction::MoveRight);
        assert_eq!(core.selected_index, 0);
    }

    #[test]
    fn grid_left_wraps_within_row() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 4;
        core.apply(LauncherAction::MoveLeft);
        assert_eq!(core.selected_index, 7);
    }

    #[test]
    fn grid_down_wraps_to_top_same_column() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 9;
        core.apply(LauncherAction::MoveDown);
        assert_eq!(core.selected_index, 1);
    }

    #[test]
    fn grid_up_wraps_to_bottom_same_column() {
        let mut core = core_with_n_apps(12);
        core.set_columns(4);
        core.selected_index = 2;
        core.apply(LauncherAction::MoveUp);
        assert_eq!(core.selected_index, 10);
    }

    #[test]
    fn grid_down_into_missing_last_row_cell_wraps_to_top() {
        // 7 apps, 3 cols → rows: [0,1,2] [3,4,5] [6]. Col 2 only exists on
        // rows 0 and 1. Down from idx 5 (row 1 col 2) wraps to idx 2 (row 0
        // col 2) instead of landing on a non-existent row-2 col-2 cell.
        let mut core = core_with_n_apps(7);
        core.set_columns(3);
        core.selected_index = 5;
        core.apply(LauncherAction::MoveDown);
        assert_eq!(core.selected_index, 2);
    }

    #[test]
    fn grid_right_on_short_last_row_wraps_within_that_row() {
        // 7 apps, 3 cols → last row has only idx 6. Right from 6 wraps to 6
        // itself (single-cell row).
        let mut core = core_with_n_apps(7);
        core.set_columns(3);
        core.selected_index = 6;
        core.apply(LauncherAction::MoveRight);
        assert_eq!(core.selected_index, 6);
    }

    #[test]
    fn list_mode_left_right_is_noop() {
        let mut core = core_with_n_apps(5);
        core.set_columns(1);
        core.selected_index = 2;
        core.apply(LauncherAction::MoveLeft);
        assert_eq!(core.selected_index, 2);
        core.apply(LauncherAction::MoveRight);
        assert_eq!(core.selected_index, 2);
    }

    #[test]
    fn empty_query_places_most_used_first() {
        let mut core = core_with_apps();
        core.scores.insert("app-c".to_string(), 42.0);
        core.rebuild_filtered();

        let (top_idx, _) = core.filtered[0];
        assert_eq!(core.apps[top_idx].id, "app-c");
    }

    #[test]
    fn view_reports_empty_reason_no_apps_when_query_empty() {
        let core = LauncherCore::for_test(vec![], Config::default());
        let view = core.view();
        assert!(view.entries.is_empty());
        assert_eq!(view.empty_reason, Some(EmptyReason::NoApps));
    }

    #[test]
    fn view_reports_no_matches_when_query_excludes_all() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::Insert('z'));
        core.apply(LauncherAction::Insert('z'));
        let view = core.view();
        assert!(view.entries.is_empty());
        assert_eq!(view.empty_reason, Some(EmptyReason::NoMatches));
    }

    #[test]
    fn view_marks_selected_entry() {
        let mut core = core_with_apps();
        core.selected_index = 1;
        let view = core.view();
        assert_eq!(view.entries.len(), 3);
        assert!(view.entries[1].selected);
        assert!(!view.entries[0].selected);
        assert_eq!(view.entries[0].name, "App A");
    }

    #[test]
    fn apply_is_noop_once_stopped() {
        let mut core = core_with_apps();
        core.apply(LauncherAction::Cancel);
        assert!(!core.running);
        core.apply(LauncherAction::Insert('a'));
        assert_eq!(core.query, "");
    }
}
