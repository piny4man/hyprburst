//! Benchmark harness and shared instrumentation contract for the Freya
//! bake-off.
//!
//! The *contract* — an injectable [`Clock`], a deterministic [`synthetic_apps`]
//! set, a [`scripted_input`] sequence of frontend-agnostic [`LauncherAction`]s,
//! the [`measure_frames`] driver, [`summarize`], and the [`Footprint`] probes —
//! is variant-agnostic: it operates purely on [`LauncherCore`] and produces a
//! [`Metrics`] record. Each frontend (baseline TUI here; the Freya POCs in later
//! phases) plugs in its own paint closure. Output is a single diffable
//! comparison table ([`render_table`]) plus a machine-readable record
//! ([`to_json`]), one column per variant.
//!
//! The baseline TUI column is populated by [`run_baseline`], which paints frames
//! headlessly into a ratatui [`Buffer`] via the shipped [`render_core`] path —
//! no terminal required, so it runs in the default build with no Freya feature.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::config::Config;
use crate::desktop::DesktopEntry;
use crate::launcher::render_core;
use crate::launcher_core::{LauncherAction, LauncherCore};

/// Per-frame budget for 60 fps; frames slower than this count as jank.
pub const JANK_BUDGET_NS: u64 = 16_666_667;

/// Monotonic time source, abstracted so the timing logic can be driven by a
/// scripted fake clock in tests.
pub trait Clock {
    /// Nanoseconds since some fixed, monotonic origin.
    fn now_nanos(&self) -> u64;
}

/// Real wall-clock source backed by [`Instant`].
pub struct MonotonicClock {
    base: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now_nanos(&self) -> u64 {
        self.base.elapsed().as_nanos() as u64
    }
}

/// Raw per-frame timings captured by [`measure_frames`], before aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTimings {
    /// Exec-equivalent → first painted frame, in nanoseconds.
    pub cold_start_ns: u64,
    /// Action → repainted frame, one entry per scripted input, in nanoseconds.
    pub frame_ns: Vec<u64>,
}

/// Footprint measurements that don't come from frame timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Footprint {
    /// Peak resident set size in KB (`/proc/self/status` `VmHWM`).
    pub peak_rss_kb: Option<u64>,
    /// On-disk size of the running binary in bytes.
    pub binary_size_bytes: Option<u64>,
    /// Number of resolved dependencies (`[[package]]` entries in `Cargo.lock`).
    pub dep_count: Option<u32>,
}

/// One variant's column in the comparison table.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub variant: String,
    pub cold_start_ns: u64,
    /// Warm hide→show→painted latency. `None` for the spawn-per-launch TUI,
    /// which has no persistent window to toggle; rendered as `N/A`.
    pub warm_toggle_ns: Option<u64>,
    /// Mean action → repainted-frame latency.
    pub input_latency_ns: u64,
    /// Sustained frames per second across the scripted input sequence.
    pub fps: f64,
    /// Frames slower than [`JANK_BUDGET_NS`].
    pub jank_count: u32,
    pub footprint: Footprint,
}

/// Deterministic in-repo app set used by every variant, so benchmark runs are
/// comparable across machines and time rather than depending on live
/// `discover_apps()`.
pub fn synthetic_apps() -> Vec<DesktopEntry> {
    const APPS: &[(&str, &str)] = &[
        ("Firefox", "firefox"),
        ("Chromium", "chromium"),
        ("Kitty", "kitty"),
        ("Alacritty", "alacritty"),
        ("Visual Studio Code", "code"),
        ("Neovim", "nvim"),
        ("GIMP", "gimp"),
        ("Inkscape", "inkscape"),
        ("Blender", "blender"),
        ("Spotify", "spotify"),
        ("Discord", "discord"),
        ("Slack", "slack"),
        ("Telegram", "telegram-desktop"),
        ("Thunderbird", "thunderbird"),
        ("LibreOffice Writer", "libreoffice-writer"),
        ("LibreOffice Calc", "libreoffice-calc"),
        ("Files", "nautilus"),
        ("Calculator", "gnome-calculator"),
        ("Steam", "steam"),
        ("OBS Studio", "obs"),
        ("VLC", "vlc"),
        ("mpv", "mpv"),
        ("Krita", "krita"),
        ("Audacity", "audacity"),
    ];
    APPS.iter()
        .map(|&(name, exec)| DesktopEntry {
            id: exec.to_string(),
            name: name.to_string(),
            icon: exec.to_string(),
            exec: exec.to_string(),
        })
        .collect()
}

/// Fixed scripted input that exercises fast typing and scrolling. Deliberately
/// excludes `LaunchSelected`/`Cancel`, which would stop the core mid-run.
pub fn scripted_input() -> Vec<LauncherAction> {
    use LauncherAction::*;
    vec![
        Insert('f'),
        Insert('i'),
        Insert('r'),
        Backspace,
        Backspace,
        Backspace,
        MoveDown,
        MoveDown,
        MoveDown,
        MoveDown,
        MoveDown,
        MoveUp,
        MoveUp,
        PageDown,
        PageUp,
        MoveRight,
        MoveLeft,
        Autocomplete,
    ]
}

/// Drive the core through cold start and the scripted input, timing each step
/// with `clock` and painting with `render`. Variant-agnostic: the caller
/// supplies how a frame is painted.
///
/// Reads the clock a deterministic `2 + 2 * input.len()` times, which is what
/// makes the timing testable with a scripted fake clock.
pub fn measure_frames(
    clock: &dyn Clock,
    core: &mut LauncherCore,
    input: &[LauncherAction],
    mut render: impl FnMut(&mut LauncherCore),
) -> RawTimings {
    let t0 = clock.now_nanos();
    render(core);
    let t1 = clock.now_nanos();
    let cold_start_ns = t1.saturating_sub(t0);

    let mut frame_ns = Vec::with_capacity(input.len());
    for &action in input {
        let before = clock.now_nanos();
        core.apply(action);
        render(core);
        let after = clock.now_nanos();
        frame_ns.push(after.saturating_sub(before));
    }

    RawTimings {
        cold_start_ns,
        frame_ns,
    }
}

/// Aggregate raw timings into a labelled [`Metrics`] column.
pub fn summarize(raw: &RawTimings, variant: &str, footprint: Footprint) -> Metrics {
    let n = raw.frame_ns.len() as u64;
    let total: u64 = raw.frame_ns.iter().sum();
    let input_latency_ns = total.checked_div(n).unwrap_or(0);
    let fps = if total == 0 {
        0.0
    } else {
        n as f64 / (total as f64 / 1_000_000_000.0)
    };
    let jank_count = raw.frame_ns.iter().filter(|&&f| f > JANK_BUDGET_NS).count() as u32;

    Metrics {
        variant: variant.to_string(),
        cold_start_ns: raw.cold_start_ns,
        warm_toggle_ns: None,
        input_latency_ns,
        fps,
        jank_count,
        footprint,
    }
}

/// Probe live footprint of the running process.
pub fn live_footprint() -> Footprint {
    Footprint {
        peak_rss_kb: peak_rss_kb(),
        binary_size_bytes: binary_size_bytes(),
        dep_count: dep_count(),
    }
}

#[cfg(target_os = "linux")]
pub fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse().ok())
    })
}

#[cfg(not(target_os = "linux"))]
pub fn peak_rss_kb() -> Option<u64> {
    None
}

fn binary_size_bytes() -> Option<u64> {
    let exe = std::env::current_exe().ok()?;
    std::fs::metadata(exe).ok().map(|m| m.len())
}

fn dep_count() -> Option<u32> {
    let lock = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock");
    std::fs::read_to_string(lock)
        .ok()
        .map(|text| dep_count_from_str(&text))
}

/// Count `[[package]]` entries in a `Cargo.lock`.
pub fn dep_count_from_str(lock: &str) -> u32 {
    lock.lines().filter(|l| l.trim() == "[[package]]").count() as u32
}

/// Run the baseline ratatui TUI variant against the contract and produce its
/// fully-populated [`Metrics`] column.
pub fn run_baseline() -> Metrics {
    let clock = MonotonicClock::new();
    let area = Rect::new(0, 0, 80, 40);
    let mut buf = Buffer::empty(area);
    let mut core = LauncherCore::from_apps(synthetic_apps(), Config::default());
    let input = scripted_input();

    let raw = measure_frames(&clock, &mut core, &input, |c| {
        render_core(c, area, &mut buf);
    });

    summarize(&raw, "baseline-tui", live_footprint())
}

/// Run the baseline and render both the human-readable table and the
/// machine-readable record.
pub fn run_baseline_report() -> String {
    let metrics = [run_baseline()];
    format!("{}\n\n{}\n", render_table(&metrics), to_json(&metrics))
}

/// Formats one metric into its table cell.
type CellFmt = fn(&Metrics) -> String;

/// Ordered (label, cell-formatter) rows of the comparison table.
fn table_rows() -> [(&'static str, CellFmt); 8] {
    [
        ("cold start", |m| fmt_ms(m.cold_start_ns)),
        ("warm toggle", |m| {
            m.warm_toggle_ns.map_or_else(|| "N/A".to_string(), fmt_ms)
        }),
        ("input latency", |m| fmt_ms(m.input_latency_ns)),
        ("fps", |m| format!("{:.1}", m.fps)),
        ("jank frames", |m| m.jank_count.to_string()),
        ("peak RSS", |m| {
            m.footprint
                .peak_rss_kb
                .map_or_else(|| "—".to_string(), |k| format!("{} KB", k))
        }),
        ("binary size", |m| {
            m.footprint
                .binary_size_bytes
                .map_or_else(|| "—".to_string(), fmt_mb)
        }),
        ("dep count", |m| {
            m.footprint
                .dep_count
                .map_or_else(|| "—".to_string(), |d| d.to_string())
        }),
    ]
}

/// Render a stable, diffable comparison table — one column per variant.
pub fn render_table(metrics: &[Metrics]) -> String {
    let rows = table_rows();

    let label_w = rows
        .iter()
        .map(|(label, _)| width(label))
        .chain(std::iter::once(width("metric")))
        .max()
        .unwrap_or(0);

    let col_w: Vec<usize> = metrics
        .iter()
        .map(|m| {
            let cells = rows.iter().map(|(_, f)| width(&f(m))).max().unwrap_or(0);
            width(&m.variant).max(cells)
        })
        .collect();

    let mut out = String::new();
    push_row(&mut out, "metric", label_w, &variant_cells(metrics), &col_w);

    out.push_str(&"-".repeat(label_w));
    for w in &col_w {
        out.push_str("-+-");
        out.push_str(&"-".repeat(*w));
    }
    out.push('\n');

    for (label, f) in rows {
        let cells: Vec<String> = metrics.iter().map(f).collect();
        push_row(&mut out, label, label_w, &cells, &col_w);
    }

    out
}

fn variant_cells(metrics: &[Metrics]) -> Vec<String> {
    metrics.iter().map(|m| m.variant.clone()).collect()
}

/// Push one `label | cell | cell …` line. Every column is left-padded to its
/// width except the last, which is left ragged to avoid trailing whitespace.
fn push_row(out: &mut String, label: &str, label_w: usize, cells: &[String], col_w: &[usize]) {
    out.push_str(&pad(label, label_w));
    let last = cells.len().saturating_sub(1);
    for (i, cell) in cells.iter().enumerate() {
        out.push_str(" | ");
        if i == last {
            out.push_str(cell);
        } else {
            out.push_str(&pad(cell, col_w[i]));
        }
    }
    out.push('\n');
}

fn pad(s: &str, w: usize) -> String {
    let len = width(s);
    if len >= w {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(w - len))
    }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

fn fmt_ms(ns: u64) -> String {
    format!("{:.3} ms", ns as f64 / 1_000_000.0)
}

fn fmt_mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_000_000.0)
}

/// Machine-readable record: a JSON array with one object per variant. Field
/// order is fixed so the output is stable and diffable.
pub fn to_json(metrics: &[Metrics]) -> String {
    let objects: Vec<String> = metrics.iter().map(metric_to_json).collect();
    format!("[{}]", objects.join(","))
}

fn metric_to_json(m: &Metrics) -> String {
    format!(
        "{{\"variant\":\"{}\",\"cold_start_ns\":{},\"warm_toggle_ns\":{},\"input_latency_ns\":{},\"fps\":{:.4},\"jank_count\":{},\"peak_rss_kb\":{},\"binary_size_bytes\":{},\"dep_count\":{}}}",
        json_escape(&m.variant),
        m.cold_start_ns,
        json_opt(m.warm_toggle_ns),
        m.input_latency_ns,
        m.fps,
        m.jank_count,
        json_opt(m.footprint.peak_rss_kb),
        json_opt(m.footprint.binary_size_bytes),
        json_opt(m.footprint.dep_count.map(u64::from)),
    )
}

fn json_opt(v: Option<u64>) -> String {
    v.map_or_else(|| "null".to_string(), |n| n.to_string())
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeClock {
        readings: Vec<u64>,
        idx: Cell<usize>,
    }

    impl FakeClock {
        fn new(readings: Vec<u64>) -> Self {
            Self {
                readings,
                idx: Cell::new(0),
            }
        }
    }

    impl Clock for FakeClock {
        fn now_nanos(&self) -> u64 {
            let i = self.idx.get();
            self.idx.set(i + 1);
            self.readings[i]
        }
    }

    fn sample_metrics() -> Metrics {
        Metrics {
            variant: "baseline-tui".to_string(),
            cold_start_ns: 1_234_000,
            warm_toggle_ns: None,
            input_latency_ns: 50_000,
            fps: 60.0,
            jank_count: 0,
            footprint: Footprint {
                peak_rss_kb: Some(7388),
                binary_size_bytes: Some(4_200_000),
                dep_count: Some(87),
            },
        }
    }

    #[test]
    fn synthetic_apps_is_deterministic_and_nonempty() {
        let a = synthetic_apps();
        let b = synthetic_apps();
        assert!(!a.is_empty());
        let ids_a: Vec<&str> = a.iter().map(|e| e.id.as_str()).collect();
        let ids_b: Vec<&str> = b.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids_a, ids_b);
    }

    #[test]
    fn scripted_input_exercises_typing_and_scroll_without_stopping() {
        let seq = scripted_input();
        assert!(seq.iter().any(|a| matches!(a, LauncherAction::Insert(_))));
        assert!(seq.iter().any(|a| matches!(a, LauncherAction::MoveDown)));
        assert!(seq.iter().any(|a| matches!(a, LauncherAction::PageDown)));
        assert!(
            !seq.iter()
                .any(|a| matches!(a, LauncherAction::LaunchSelected | LauncherAction::Cancel)),
            "benchmark input must not launch or cancel the core",
        );
    }

    #[test]
    fn measure_frames_brackets_each_step_with_the_clock() {
        let mut core = LauncherCore::from_apps(synthetic_apps(), Config::default());
        let input = vec![LauncherAction::Insert('a'), LauncherAction::MoveDown];
        // Reads: t0, t1 (cold), then before/after for each of 2 frames.
        let clock = FakeClock::new(vec![0, 100, 200, 350, 400, 600]);

        let raw = measure_frames(&clock, &mut core, &input, |_| {});

        assert_eq!(raw.cold_start_ns, 100);
        assert_eq!(raw.frame_ns, vec![150, 200]);
    }

    #[test]
    fn summarize_computes_latency_fps_and_jank() {
        let raw = RawTimings {
            cold_start_ns: 100,
            frame_ns: vec![10_000_000, 20_000_000],
        };
        let m = summarize(&raw, "baseline-tui", Footprint::default());

        assert_eq!(m.cold_start_ns, 100);
        assert_eq!(m.input_latency_ns, 15_000_000);
        assert_eq!(m.jank_count, 1, "20ms frame exceeds the 60fps budget");
        assert!((m.fps - 66.6666).abs() < 0.01);
        assert_eq!(m.warm_toggle_ns, None);
    }

    #[test]
    fn summarize_handles_zero_frames() {
        let raw = RawTimings {
            cold_start_ns: 0,
            frame_ns: vec![],
        };
        let m = summarize(&raw, "x", Footprint::default());

        assert_eq!(m.input_latency_ns, 0);
        assert!(m.fps.abs() < 1e-9);
        assert_eq!(m.jank_count, 0);
    }

    #[test]
    fn dep_count_from_str_counts_packages() {
        let lock = "\
[[package]]
name = \"a\"
version = \"1.0\"

[[package]]
name = \"b\"
version = \"2.0\"
";
        assert_eq!(dep_count_from_str(lock), 2);
    }

    #[test]
    fn render_table_has_stable_shape() {
        let table = render_table(&[sample_metrics()]);
        let expected = format!(
            "{lbl:<13} | {val}\n\
             {dashes}-+-{vdash}\n\
             {l1:<13} | {v1}\n\
             {l2:<13} | {v2}\n\
             {l3:<13} | {v3}\n\
             {l4:<13} | {v4}\n\
             {l5:<13} | {v5}\n\
             {l6:<13} | {v6}\n\
             {l7:<13} | {v7}\n\
             {l8:<13} | {v8}\n",
            lbl = "metric",
            val = "baseline-tui",
            dashes = "-".repeat(13),
            vdash = "-".repeat(12),
            l1 = "cold start",
            v1 = "1.234 ms",
            l2 = "warm toggle",
            v2 = "N/A",
            l3 = "input latency",
            v3 = "0.050 ms",
            l4 = "fps",
            v4 = "60.0",
            l5 = "jank frames",
            v5 = "0",
            l6 = "peak RSS",
            v6 = "7388 KB",
            l7 = "binary size",
            v7 = "4.20 MB",
            l8 = "dep count",
            v8 = "87",
        );
        assert_eq!(table, expected);
    }

    #[test]
    fn to_json_is_stable() {
        let json = to_json(&[sample_metrics()]);
        let expected = "[{\"variant\":\"baseline-tui\",\"cold_start_ns\":1234000,\"warm_toggle_ns\":null,\"input_latency_ns\":50000,\"fps\":60.0000,\"jank_count\":0,\"peak_rss_kb\":7388,\"binary_size_bytes\":4200000,\"dep_count\":87}]";
        assert_eq!(json, expected);
    }

    #[test]
    fn run_baseline_populates_every_metric() {
        let m = run_baseline();
        assert_eq!(m.variant, "baseline-tui");
        assert!(m.cold_start_ns > 0);
        assert!(m.input_latency_ns > 0);
        assert!(m.fps > 0.0);
        assert!(m.footprint.peak_rss_kb.is_some());
        assert!(m.footprint.binary_size_bytes.is_some());
        assert!(m.footprint.dep_count.is_some());
        assert_eq!(m.warm_toggle_ns, None);
    }
}
