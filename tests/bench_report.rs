//! End-to-end check of the Phase 2 benchmark harness.
//!
//! Exercises the public `hyprburst::bench` API the way the `--bench` CLI flag
//! does, confirming the baseline TUI column is fully populated and that both the
//! human-readable table and the machine-readable record are emitted. The harness
//! is part of the default build (no Freya feature), so this test runs in the
//! standard `cargo test` lane.

use hyprburst::bench;

#[test]
fn baseline_report_emits_table_and_machine_record() {
    let report = bench::run_baseline_report();

    // Human-readable table: header, metric rows, the documented N/A warm toggle.
    assert!(
        report.contains("baseline-tui"),
        "missing variant column header"
    );
    assert!(report.contains("cold start"));
    assert!(report.contains("input latency"));
    assert!(report.contains("warm toggle"));
    assert!(
        report.contains("N/A"),
        "warm toggle should be N/A for the TUI"
    );

    // Machine-readable record.
    assert!(report.contains("\"variant\":\"baseline-tui\""));
    assert!(report.contains("\"cold_start_ns\":"));
    assert!(report.contains("\"warm_toggle_ns\":null"));
}

#[test]
fn baseline_column_is_fully_populated() {
    let m = bench::run_baseline();

    assert_eq!(m.variant, "baseline-tui");
    // Latency + smoothness.
    assert!(m.cold_start_ns > 0, "cold start not measured");
    assert!(m.input_latency_ns > 0, "input latency not measured");
    assert!(m.fps > 0.0, "fps not measured");
    // Footprint.
    assert!(m.footprint.peak_rss_kb.is_some(), "peak RSS not captured");
    assert!(
        m.footprint.binary_size_bytes.is_some(),
        "binary size not captured"
    );
    assert!(m.footprint.dep_count.is_some(), "dep count not captured");
}
