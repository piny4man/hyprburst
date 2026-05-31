//! Phase 3 viability probe for the Freya bake-off — the early kill-switch.
//!
//! A deliberately thin Freya window: it opens on Hyprland, carries the app-id
//! `hyprburst` (so the shipped windowrules apply), and stamps a first-frame
//! timestamp through the shared instrumentation contract in [`hyprburst::bench`].
//! It exists only to answer "does Freya boot in a viable ballpark and present as
//! a styleable native window?" before either full POC is built. It grows into
//! the real native-GUI launcher (`hyprburst-spike-gui`) in Phase 4.
//!
//! Built only with the `freya-spike` feature (see `required-features` in
//! `Cargo.toml`), so the default build and shipped binary never pull Freya.
//!
//! Modes:
//! - default — open the window and hold it, for visual windowrules inspection
//!   (float/fullscreen/opacity/blur). The first-frame metrics are printed once
//!   the window paints; close the window to exit.
//! - `--measure` — same, but the process exits right after the first frame is
//!   captured, so the harness can read cold-start and peak RSS without a human
//!   in the loop.
//!
//! Cold-start here is measured from process start to the app root's first render
//! (the closest cheap proxy for the first painted frame); a short settle lets the
//! GPU surface allocate before peak RSS is read.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use freya::prelude::*;
use hyprburst::bench;

/// Wayland app-id — must match the id the shipped windowrules target.
const APP_ID: &str = "hyprburst";
/// Column label this variant fills in the comparison table.
const VARIANT: &str = "freya-window-probe";

/// Process-start reference, set once at the top of `main`.
static START: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds from process start to the first app render (0 = not yet painted).
static FIRST_FRAME_NS: AtomicU64 = AtomicU64::new(0);

fn main() {
    let _ = START.set(Instant::now());
    let measure = std::env::args().skip(1).any(|a| a == "--measure");

    // Report (and, in --measure, exit) once the first frame lands. Runs off the
    // UI thread so it never blocks Freya's event loop.
    spawn_reporter(measure);

    launch(
        LaunchConfig::new().with_exit_on_close(true).with_window(
            WindowConfig::new(app)
                .with_app_id(APP_ID)
                .with_title("hyprburst (freya spike)")
                .with_size(640.0, 720.0)
                .with_transparency(true)
                .with_background((20, 20, 28)),
        ),
    );
}

/// App root. Stamps the first-frame timestamp on its first render (via
/// [`use_hook`], which runs its initializer exactly once) and draws a minimal
/// centered banner so the window is visible for windowrules inspection.
fn app() -> impl IntoElement {
    use_hook(|| {
        if let Some(start) = START.get() {
            let ns = (start.elapsed().as_nanos() as u64).max(1);
            // Only the first stamp counts; ignore if already set.
            let _ = FIRST_FRAME_NS.compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst);
        }
    });

    rect()
        .expanded()
        .center()
        .background((20, 20, 28))
        .color((235, 235, 245))
        .font_size(28.0)
        .child("hyprburst · freya spike")
}

/// Wait for the first frame, then emit the probe's metrics column. With
/// `exit_after`, terminate the process right after — clean cold-start/RSS
/// capture for the harness. Bails out after a timeout if no frame ever paints
/// (e.g. launched with no Wayland display).
fn spawn_reporter(exit_after: bool) {
    std::thread::spawn(move || {
        loop {
            let ns = FIRST_FRAME_NS.load(Ordering::SeqCst);
            if ns != 0 {
                // Let the GPU surface actually paint and settle before we read RSS.
                std::thread::sleep(Duration::from_millis(50));
                report(ns);
                if exit_after {
                    std::process::exit(0);
                }
                return;
            }
            if START
                .get()
                .is_some_and(|s| s.elapsed() > Duration::from_secs(10))
            {
                eprintln!(
                    "hyprburst-spike-gui: no frame within 10s — is a Wayland display available?"
                );
                if exit_after {
                    std::process::exit(1);
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// Print the probe column: the human-readable table plus the machine-readable
/// record, reusing the exact contract the baseline TUI column uses.
fn report(cold_start_ns: u64) {
    let footprint = bench::live_footprint_for(&["freya-spike"]);
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}
