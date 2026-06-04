//! Footprint probes and the `--measure` report for the launcher.
//!
//! What survives the bake-off: the live instrumentation the shipped binary still
//! uses. [`peak_rss_kb`] backs `hyprburst --bench-startup`; [`probe_metrics`] +
//! [`live_footprint`] + [`render_table`] + [`to_json`] back `hyprburst --measure`
//! (the GPU window reports its cold-start and footprint at first paint). The
//! multi-variant comparison harness was removed once the GPU launcher won.

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

/// One variant's column in the report table.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub variant: String,
    pub cold_start_ns: u64,
    /// Warm hide→show→painted latency. `None` for the spawn-per-launch launcher,
    /// which has no persistent window to toggle; rendered as `N/A`.
    pub warm_toggle_ns: Option<u64>,
    /// Mean action → repainted-frame latency.
    pub input_latency_ns: u64,
    /// Sustained frames per second across the scripted input sequence.
    pub fps: f64,
    /// Frames slower than the 60 fps budget.
    pub jank_count: u32,
    pub footprint: Footprint,
}

/// Build a [`Metrics`] column for the GPU window's first-paint report.
///
/// Only cold-start (process start → first painted frame) and footprint are
/// measured: the report is emitted at first paint, before any input loop, so
/// input latency, fps, and jank stay zero, and warm-toggle is `N/A` (the launcher
/// spawns per launch).
pub fn probe_metrics(variant: &str, cold_start_ns: u64, footprint: Footprint) -> Metrics {
    Metrics {
        variant: variant.to_string(),
        cold_start_ns,
        warm_toggle_ns: None,
        input_latency_ns: 0,
        fps: 0.0,
        jank_count: 0,
        footprint,
    }
}

/// Probe live footprint of the running process (default-feature dep count).
pub fn live_footprint() -> Footprint {
    live_footprint_for(&[])
}

/// Probe live footprint, attributing the dep count to the build selected by
/// the given extra cargo `features`. The default build passes no features; the
/// parameter is retained for forward use if optional feature columns return.
pub fn live_footprint_for(features: &[&str]) -> Footprint {
    Footprint {
        peak_rss_kb: peak_rss_kb(),
        binary_size_bytes: binary_size_bytes(),
        dep_count: dep_count_for(features),
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

/// Count the dependencies actually pulled into the build for the given extra
/// cargo `features`, via `cargo tree`. The default build passes no features.
/// Falls back to the `Cargo.lock` package count when `cargo` is unavailable
/// (e.g. a shipped binary on a machine with no toolchain).
pub fn dep_count_for(features: &[&str]) -> Option<u32> {
    cargo_tree(features)
        .map(|tree| dep_count_from_tree(&tree))
        .or_else(dep_count_from_lock)
}

/// Run `cargo tree --prefix none` for the given extra features and return its
/// stdout. `None` if `cargo` is missing or the command fails.
fn cargo_tree(features: &[&str]) -> Option<String> {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["tree", "--prefix", "none", "--manifest-path", manifest]);
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Read the `Cargo.lock` union package count — fallback for [`dep_count_for`]
/// when `cargo tree` can't run.
fn dep_count_from_lock() -> Option<u32> {
    let lock = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock");
    std::fs::read_to_string(lock)
        .ok()
        .map(|text| dep_count_from_str(&text))
}

/// Count `[[package]]` entries in a `Cargo.lock` (the union across every
/// optional feature).
pub fn dep_count_from_str(lock: &str) -> u32 {
    lock.lines().filter(|l| l.trim() == "[[package]]").count() as u32
}

/// Count unique `name vVERSION` packages in `cargo tree --prefix none` output,
/// de-duplicating repeated subtrees (`(*)`) and ignoring trailing annotations
/// such as `(proc-macro)` or the root crate's source path.
pub fn dep_count_from_tree(tree: &str) -> u32 {
    let mut seen = std::collections::BTreeSet::new();
    for line in tree.lines() {
        let mut tokens = line.split_whitespace();
        if let (Some(name), Some(version)) = (tokens.next(), tokens.next())
            && version.starts_with('v')
        {
            seen.insert((name.to_string(), version.to_string()));
        }
    }
    seen.len() as u32
}

/// Formats one metric into its table cell.
type CellFmt = fn(&Metrics) -> String;

/// Ordered (label, cell-formatter) rows of the report table.
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

/// Render a stable, diffable report table — one column per variant.
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
    fn dep_count_from_tree_counts_unique_packages() {
        // Mirrors `cargo tree --prefix none`: a root with a source path,
        // `(proc-macro)` and `(*)` annotations, a duplicate subtree, and a
        // blank line — all of which must collapse to the unique package set.
        let tree = "\
hyprburst v0.4.3 (/home/u/hyprburst)
crossterm v0.29.0
derive_more-impl v2.1.1 (proc-macro)
proc-macro2 v1.0.106

crossterm v0.29.0 (*)
ratatui v0.30.0
";
        // hyprburst, crossterm, derive_more-impl, proc-macro2, ratatui = 5.
        assert_eq!(dep_count_from_tree(tree), 5);
    }

    #[test]
    fn probe_metrics_measures_cold_start_and_footprint_only() {
        let footprint = Footprint {
            peak_rss_kb: Some(42_000),
            binary_size_bytes: Some(120_000_000),
            dep_count: Some(375),
        };
        let m = probe_metrics("gui", 9_000_000, footprint);

        assert_eq!(m.variant, "gui");
        assert_eq!(m.cold_start_ns, 9_000_000);
        assert_eq!(m.footprint, footprint);
        // First-paint report: no input loop, so these stay unmeasured.
        assert_eq!(m.input_latency_ns, 0);
        assert!(m.fps.abs() < 1e-9);
        assert_eq!(m.jank_count, 0);
        assert_eq!(m.warm_toggle_ns, None);
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
}
