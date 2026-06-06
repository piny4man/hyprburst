use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

const HALF_LIFE_SECS: f64 = 7.0 * 86400.0;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub desktop_id: String,
    pub app_name: String,
    pub launch_count: u64,
    pub last_used: i64,
    pub first_used: i64,
}

pub struct History {
    conn: Connection,
}

#[allow(dead_code)]
impl History {
    pub fn open() -> rusqlite::Result<Self> {
        let path = default_db_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self::open_at(&path)
    }

    pub fn open_at(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn record_launch(&self, desktop_id: &str, app_name: &str) -> rusqlite::Result<()> {
        self.record_launch_at(desktop_id, app_name, now_unix())
    }

    pub fn record_launch_at(
        &self,
        desktop_id: &str,
        app_name: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO launches (desktop_id, app_name, launch_count, last_used, first_used)
             VALUES (?1, ?2, 1, ?3, ?3)
             ON CONFLICT(desktop_id) DO UPDATE SET
               launch_count = launch_count + 1,
               last_used = excluded.last_used,
               app_name = excluded.app_name",
            params![desktop_id, app_name, now],
        )?;
        Ok(())
    }

    pub fn get(&self, desktop_id: &str) -> rusqlite::Result<Option<HistoryEntry>> {
        self.conn
            .query_row(
                "SELECT desktop_id, app_name, launch_count, last_used, first_used
                 FROM launches WHERE desktop_id = ?1",
                params![desktop_id],
                row_to_entry,
            )
            .optional()
    }

    pub fn all(&self) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT desktop_id, app_name, launch_count, last_used, first_used FROM launches",
        )?;
        let rows = stmt.query_map([], row_to_entry)?;
        rows.collect()
    }

    pub fn total_launches(&self) -> rusqlite::Result<u64> {
        let total: Option<i64> = self.conn.query_row(
            "SELECT COALESCE(SUM(launch_count), 0) FROM launches",
            [],
            |row| row.get(0),
        )?;
        Ok(total.unwrap_or(0).max(0) as u64)
    }

    pub fn top_apps(&self, n: usize) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT desktop_id, app_name, launch_count, last_used, first_used
             FROM launches
             ORDER BY launch_count DESC, last_used DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![n as i64], row_to_entry)?;
        rows.collect()
    }
}

pub fn score(entry: &HistoryEntry, now: i64) -> f64 {
    if entry.launch_count == 0 {
        return 0.0;
    }
    let age = (now - entry.last_used).max(0) as f64;
    let decay = (-age * std::f64::consts::LN_2 / HALF_LIFE_SECS).exp();
    entry.launch_count as f64 * decay
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        desktop_id: row.get(0)?,
        app_name: row.get(1)?,
        launch_count: row.get::<_, i64>(2)?.max(0) as u64,
        last_used: row.get(3)?,
        first_used: row.get(4)?,
    })
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS launches (
             desktop_id TEXT PRIMARY KEY,
             app_name TEXT NOT NULL,
             launch_count INTEGER NOT NULL DEFAULT 0,
             last_used INTEGER NOT NULL,
             first_used INTEGER NOT NULL
         )",
        [],
    )?;
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn default_db_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("hyprburst").join("history.db");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local/share/hyprburst")
            .join("history.db");
    }
    PathBuf::from("hyprburst_history.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> History {
        History::in_memory().expect("open in-memory history")
    }

    #[test]
    fn in_memory_creates_schema() {
        let h = open_in_memory();
        assert_eq!(h.total_launches().unwrap(), 0);
        assert!(h.all().unwrap().is_empty());
    }

    #[test]
    fn record_launch_inserts_new_entry() {
        let h = open_in_memory();
        h.record_launch_at("firefox", "Firefox", 1000).unwrap();

        let entry = h.get("firefox").unwrap().unwrap();
        assert_eq!(entry.desktop_id, "firefox");
        assert_eq!(entry.app_name, "Firefox");
        assert_eq!(entry.launch_count, 1);
        assert_eq!(entry.first_used, 1000);
        assert_eq!(entry.last_used, 1000);
    }

    #[test]
    fn record_launch_increments_count_and_updates_last_used() {
        let h = open_in_memory();
        h.record_launch_at("firefox", "Firefox", 1000).unwrap();
        h.record_launch_at("firefox", "Firefox", 2000).unwrap();
        h.record_launch_at("firefox", "Firefox", 3000).unwrap();

        let entry = h.get("firefox").unwrap().unwrap();
        assert_eq!(entry.launch_count, 3);
        assert_eq!(entry.first_used, 1000);
        assert_eq!(entry.last_used, 3000);
    }

    #[test]
    fn record_launch_updates_app_name_on_rename() {
        let h = open_in_memory();
        h.record_launch_at("firefox", "Firefox", 1000).unwrap();
        h.record_launch_at("firefox", "Firefox Nightly", 2000)
            .unwrap();

        let entry = h.get("firefox").unwrap().unwrap();
        assert_eq!(entry.app_name, "Firefox Nightly");
    }

    #[test]
    fn get_returns_none_for_missing() {
        let h = open_in_memory();
        assert!(h.get("missing").unwrap().is_none());
    }

    #[test]
    fn all_returns_all_entries() {
        let h = open_in_memory();
        h.record_launch_at("a", "A", 1).unwrap();
        h.record_launch_at("b", "B", 2).unwrap();
        h.record_launch_at("c", "C", 3).unwrap();

        let entries = h.all().unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn total_launches_sums_all() {
        let h = open_in_memory();
        h.record_launch_at("a", "A", 1).unwrap();
        h.record_launch_at("a", "A", 2).unwrap();
        h.record_launch_at("b", "B", 3).unwrap();

        assert_eq!(h.total_launches().unwrap(), 3);
    }

    #[test]
    fn total_launches_empty_db() {
        let h = open_in_memory();
        assert_eq!(h.total_launches().unwrap(), 0);
    }

    #[test]
    fn top_apps_sorted_by_count() {
        let h = open_in_memory();
        for _ in 0..5 {
            h.record_launch_at("firefox", "Firefox", 100).unwrap();
        }
        for _ in 0..2 {
            h.record_launch_at("chrome", "Chrome", 200).unwrap();
        }
        h.record_launch_at("vim", "Vim", 300).unwrap();

        let top = h.top_apps(10).unwrap();
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].desktop_id, "firefox");
        assert_eq!(top[1].desktop_id, "chrome");
        assert_eq!(top[2].desktop_id, "vim");
    }

    #[test]
    fn top_apps_respects_limit() {
        let h = open_in_memory();
        for i in 0..5 {
            h.record_launch_at(&format!("app{}", i), "A", i).unwrap();
        }
        let top = h.top_apps(3).unwrap();
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn top_apps_empty_db() {
        let h = open_in_memory();
        assert!(h.top_apps(10).unwrap().is_empty());
    }

    #[test]
    fn persists_across_reopens() {
        let tmp = tempdir();
        let path = tmp.join("history.db");
        {
            let h = History::open_at(&path).unwrap();
            h.record_launch_at("firefox", "Firefox", 1000).unwrap();
        }
        let h = History::open_at(&path).unwrap();
        let entry = h.get("firefox").unwrap().unwrap();
        assert_eq!(entry.launch_count, 1);
    }

    #[test]
    fn score_zero_for_zero_count() {
        let entry = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 0,
            last_used: 1000,
            first_used: 1000,
        };
        assert_eq!(score(&entry, 1000), 0.0);
    }

    #[test]
    fn score_equals_count_when_just_used() {
        let entry = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 5,
            last_used: 1000,
            first_used: 500,
        };
        let s = score(&entry, 1000);
        assert!((s - 5.0).abs() < 1e-9);
    }

    #[test]
    fn score_halves_after_half_life() {
        let entry = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 10,
            last_used: 0,
            first_used: 0,
        };
        let s = score(&entry, HALF_LIFE_SECS as i64);
        assert!((s - 5.0).abs() < 0.01, "expected ~5.0, got {}", s);
    }

    #[test]
    fn score_decays_with_age() {
        let base = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 10,
            last_used: 0,
            first_used: 0,
        };
        let recent = score(&base, 10);
        let older = score(&base, 86400);
        let ancient = score(&base, 86400 * 30);
        assert!(recent > older);
        assert!(older > ancient);
    }

    #[test]
    fn score_higher_for_frequent() {
        let freq = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 20,
            last_used: 1000,
            first_used: 0,
        };
        let rare = HistoryEntry {
            desktop_id: "y".into(),
            app_name: "Y".into(),
            launch_count: 1,
            last_used: 1000,
            first_used: 0,
        };
        assert!(score(&freq, 1000) > score(&rare, 1000));
    }

    #[test]
    fn score_handles_future_last_used() {
        let entry = HistoryEntry {
            desktop_id: "x".into(),
            app_name: "X".into(),
            launch_count: 3,
            last_used: 2000,
            first_used: 1000,
        };
        let s = score(&entry, 1000);
        assert!((s - 3.0).abs() < 1e-9);
    }

    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "hyprburst-history-test-{}-{}",
            std::process::id(),
            test_counter()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn test_counter() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}
