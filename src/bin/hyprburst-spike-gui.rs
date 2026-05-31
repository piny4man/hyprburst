//! Phase 4 native-GUI POC for the Freya bake-off.
//!
//! Grown from the Phase 3 viability probe into the real launcher: a Freya window
//! that drives the shared [`hyprburst::launcher_core::LauncherCore`] and renders
//! its view — banner, search prompt, and an app grid — so it looks and behaves
//! like the shipped TUI, on a GPU/Skia surface. Freya keyboard events map to the
//! abstract `LauncherAction` vocabulary via [`hyprburst::gui::key_to_action`];
//! all selection, filtering, and launching stay in the core.
//!
//! Built only with the `freya-spike` feature (see `required-features` in
//! `Cargo.toml`), so the default build and shipped binary never pull Freya.
//!
//! Modes:
//! - default — open the window and run the launcher interactively (type to
//!   filter, arrows to navigate the grid, Enter launches via the core, Esc
//!   cancels). The first-frame metrics print once the window paints.
//! - `--measure` — same window, but the process exits right after the first
//!   frame is captured, for unattended cold-start / peak-RSS capture.
//! - `--bench` — no window: run the harness headlessly and print the baseline
//!   vs. native-GUI comparison table (cold-start, input latency, fps/jank,
//!   footprint), then exit.
//! - `--icons` — no window: print the Phase 6 glyph-vs-themed-icon delta (the
//!   non-gating measured bonus) with a one-line viability read, then exit.
//!
//! The live render honors `HYPRBURST_GUI_ICONS=themed` to draw real themed icon
//! images instead of Nerd Font glyphs (Phase 6).
//!
//! Cold-start here is measured from process start to the app root's first
//! render; a short settle lets the GPU surface allocate before peak RSS is read.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use freya::prelude::*;
use hyprburst::bench;
use hyprburst::config::Config;
use hyprburst::gui;
use hyprburst::launcher_core::LauncherCore;

/// Wayland app-id — must match the id the shipped windowrules target.
const APP_ID: &str = "hyprburst";
/// Column label this variant fills in the comparison table.
const VARIANT: &str = "native-gui";

/// Process-start reference, set once at the top of `main`.
static START: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds from process start to the first app render (0 = not yet painted).
static FIRST_FRAME_NS: AtomicU64 = AtomicU64::new(0);

fn main() {
    let _ = START.set(Instant::now());
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Headless benchmark mode: no window, just the comparison table.
    if args.iter().any(|a| a == "--bench") {
        print!("{}", bench::run_bake_off_report());
        return;
    }

    // Phase 6 measured bonus: glyph vs themed-icon delta, headless.
    if args.iter().any(|a| a == "--icons") {
        print!("{}", bench::run_icon_delta_report());
        return;
    }

    let measure = args.iter().any(|a| a == "--measure");

    // Report (and, in --measure, exit) once the first frame lands. Runs off the
    // UI thread so it never blocks Freya's event loop.
    spawn_reporter(measure);

    launch(
        LaunchConfig::new().with_exit_on_close(true).with_window(
            WindowConfig::new(app)
                .with_app_id(APP_ID)
                .with_title("hyprburst (freya spike)")
                .with_size(f64::from(gui::WINDOW_WIDTH), f64::from(gui::WINDOW_HEIGHT))
                .with_transparency(true)
                .with_background(gui::WINDOW_CLEAR),
        ),
    );
}

/// App root. Holds the shared [`LauncherCore`] in reactive state, stamps the
/// first-frame timestamp on its first render, maps global key events to
/// [`LauncherAction`](hyprburst::launcher_core::LauncherAction)s, and renders the
/// core's view through [`gui::render_frame`].
fn app() -> impl IntoElement {
    use_hook(|| {
        if let Some(start) = START.get() {
            let ns = (start.elapsed().as_nanos() as u64).max(1);
            // Only the first stamp counts; ignore if already set.
            let _ = FIRST_FRAME_NS.compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst);
        }
    });

    let mut core = use_state(|| {
        let mut core = LauncherCore::new(load_config());
        let cols = gui::columns_for(core.config());
        core.set_columns(cols);
        core
    });

    let content = {
        let core = core.read();
        let view = core.view();
        // Icon path chosen once from the environment (`HYPRBURST_GUI_ICONS`):
        // glyph by default, themed images when opted in (Phase 6).
        let frame = gui::build_frame(&view, core.config(), gui::icon_mode());
        gui::render_frame(&frame, core.config())
    };

    rect()
        .expanded()
        .background(gui::WINDOW_CLEAR)
        .on_global_key_down(move |event: Event<KeyboardEventData>| {
            if let Some(action) = gui::key_to_action(&event.key) {
                core.write().apply(action);
                // Enter launched (via the core) or Esc cancelled — the core has
                // stopped, so tear down the window.
                if !core.read().running() {
                    std::process::exit(0);
                }
            }
        })
        .child(content)
}

/// Load the user's config for the live launcher, falling back to defaults so the
/// window always opens (errors are surfaced but never fatal here).
fn load_config() -> Config {
    match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("hyprburst-spike-gui: {err}; using defaults");
            Config::default()
        }
    }
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

/// Print the live native-GUI column: cold-start to first frame plus footprint,
/// reusing the exact contract the baseline TUI column uses. Input-latency / fps
/// for this variant come from the headless `--bench` run.
fn report(cold_start_ns: u64) {
    let footprint = bench::live_footprint_for(&["freya-spike"]);
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}
