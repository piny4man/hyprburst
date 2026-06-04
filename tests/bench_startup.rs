//! Regression guard for the `--bench-startup` path in `src/main.rs`.
//!
//! Mirrors the work that `bench_startup()` does (config load + `App::new`) and
//! asserts the elapsed time stays under a CI-friendly ceiling so we notice
//! startup regressions as soon as they land.

use std::time::Instant;

use hyprburst::domain::config::Config;
use hyprburst::tui::app::App;

// Local release target is ~4ms and the documented budget is <50ms. The ceiling
// is set well above that because the test runs in a debug build on shared CI
// runners, which can be an order of magnitude slower than a cold release run.
// If this starts flaking despite the headroom, mark the test `#[ignore]` and
// rely on the `cargo test -- --ignored` lane documented in the README.
const STARTUP_CEILING_MS: u128 = 250;

#[test]
fn bench_startup_stays_under_ceiling() {
    let start = Instant::now();
    let config = Config::load().unwrap_or_default();
    let _app = App::new(config);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < STARTUP_CEILING_MS,
        "startup took {:.2}ms, over the {}ms ceiling",
        elapsed.as_secs_f64() * 1_000.0,
        STARTUP_CEILING_MS
    );
}
