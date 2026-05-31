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
//! - `--resident` — stay alive after a launch: hide the window (via the Hyprland
//!   special workspace) and reset to a clean launcher instead of exiting, so the
//!   window can be toggled back on *warm*. `SIGUSR1` arms a warm-toggle
//!   measurement (show-trigger → first present), reported to stderr. This is the
//!   Phase 7 variant that fills the warm-toggle row.
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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use freya::prelude::*;
use hyprburst::bench;
use hyprburst::config::Config;
use hyprburst::gui;
use hyprburst::launcher_core::LauncherCore;
use hyprburst::spike_metrics::{FirstPaintPlugin, WarmToggle, WarmTogglePlugin};

/// Wayland app-id — must match the id the shipped windowrules target.
const APP_ID: &str = "hyprburst";
/// Column label this variant fills in the comparison table.
const VARIANT: &str = "native-gui";

/// Process-start reference, set once at the top of `main`.
static START: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds from process start to the app root's first component render
/// (0 = not yet rendered). Fires early — before Skia paints — so it's reported
/// only as a breakdown, not as the cold-start headline.
static FIRST_FRAME_NS: AtomicU64 = AtomicU64::new(0);
/// Nanoseconds from process start to the first frame actually *presented* to the
/// window (0 = not yet painted). This is the honest cold-start: time-to-visible.
static FIRST_PRESENT_NS: AtomicU64 = AtomicU64::new(0);
/// Warm-toggle capture (Phase 7): show-trigger → first present after re-show.
/// Armed on `SIGUSR1`, stamped by [`WarmTogglePlugin`] on the next present.
static WARM: WarmToggle = WarmToggle::new();
/// Whether the process runs resident (`--resident`): a launch hides the window
/// and resets state instead of exiting, so it can be toggled back on warm.
static RESIDENT: AtomicBool = AtomicBool::new(false);
/// Set by the `SIGUSR1` listener to ask the app root to reset to a clean
/// launcher on its next render (the re-show repaint). Swapped back to `false`
/// once consumed, so a single show triggers exactly one reset.
static NEEDS_RESET: AtomicBool = AtomicBool::new(false);

fn main() {
    let start = Instant::now();
    let _ = START.set(start);
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
    let resident = args.iter().any(|a| a == "--resident");
    RESIDENT.store(resident, Ordering::SeqCst);

    if resident {
        // Resident: report cold-start without exiting, watch for warm toggles,
        // and arm a warm-toggle measurement whenever the window is shown
        // (SIGUSR1, sent by the special-workspace bind or the manual fallback).
        spawn_reporter(false);
        spawn_warm_reporter();
        spawn_show_trigger_listener();
    } else {
        // Report (and, in --measure, exit) once the first frame lands. Runs off
        // the UI thread so it never blocks Freya's event loop.
        spawn_reporter(measure);
    }

    let window = WindowConfig::new(app)
        .with_app_id(APP_ID)
        .with_title("hyprburst (freya spike)")
        .with_size(f64::from(gui::WINDOW_WIDTH), f64::from(gui::WINDOW_HEIGHT))
        .with_transparency(true)
        .with_background(gui::WINDOW_CLEAR);

    let mut config = LaunchConfig::new()
        .with_exit_on_close(true)
        // Stamp the honest cold-start at the first real frame present.
        .with_plugin(FirstPaintPlugin::new(start, &FIRST_PRESENT_NS));
    if resident {
        // Stamp each warm re-show (show-trigger → first present after it).
        config = config.with_plugin(WarmTogglePlugin::new(start, &WARM));
    }
    launch(config.with_window(window));
}

/// Hide the resident launcher by toggling its Hyprland special workspace off.
/// Paired with `windowrule = workspace special:hyprburst, app-id:hyprburst`, this
/// is how a launch dismisses the window while the process stays alive for the
/// next warm toggle. Best-effort: a spawn failure (no Hyprland) is ignored.
fn hide_special_workspace() {
    // Use whichever `hyprctl` dispatch form the running Hyprland accepts (the
    // 0.55+ Lua form otherwise silently no-ops). See `hyprburst::hyprland`.
    hyprburst::hyprland::dispatch_toggle_special(APP_ID);
}

/// Listen for `SIGUSR1` on a dedicated thread and, on each one, arm a
/// warm-toggle measurement (timestamping the show-trigger) and request a state
/// reset for the re-show. This is the unattended `--measure`-style driver and a
/// stack-agnostic manual fallback for the special-workspace bind. Synchronous
/// delivery on its own thread keeps it free of async-signal-safety hazards.
fn spawn_show_trigger_listener() {
    use signal_hook::consts::SIGUSR1;
    use signal_hook::iterator::Signals;

    std::thread::spawn(|| match Signals::new([SIGUSR1]) {
        Ok(mut signals) => {
            for _ in signals.forever() {
                if let Some(start) = START.get() {
                    WARM.arm(start.elapsed().as_nanos() as u64);
                }
                NEEDS_RESET.store(true, Ordering::SeqCst);
            }
        }
        Err(err) => eprintln!("hyprburst-spike-gui: SIGUSR1 listener unavailable: {err}"),
    });
}

/// Print each newly-measured warm-toggle latency to stderr (resident mode). A
/// light poll — this is a measurement-reporting thread, not the render path, so
/// it never affects the launcher's own idle behavior.
fn spawn_warm_reporter() {
    std::thread::spawn(|| {
        let mut last = 0u64;
        loop {
            if let Some(ns) = WARM.latest()
                && ns != last
            {
                last = ns;
                eprintln!(
                    "hyprburst-spike-gui: warm toggle {:.3} ms (show-trigger → present)",
                    ns as f64 / 1e6,
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
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

    // Resident re-show: if the SIGUSR1 listener asked for a reset (the window was
    // just toggled back on), clear the previous query/selection before rendering
    // so the warm launcher shows fresh. Consuming the flag here — on Freya's
    // natural re-render after re-show — keeps the process idle while hidden (no
    // poll loop on the render path). The following present is what the
    // `WarmTogglePlugin` stamps as the warm-toggle latency.
    if RESIDENT.load(Ordering::SeqCst) && NEEDS_RESET.swap(false, Ordering::SeqCst) {
        core.write().reset();
    }

    // Icon mode is read from the environment (`HYPRBURST_GUI_ICONS`) once; the
    // themed path holds a persistent resolver so each icon name is resolved at
    // most once per process (resolving per frame makes the launcher unnavigable).
    let mode = use_hook(gui::icon_mode);
    let resolver = use_hook(|| Rc::new(RefCell::new(gui::IconResolver::default())));

    let content = {
        let core = core.read();
        let view = core.view();
        let frame = gui::build_frame(&view, core.config());
        if mode == gui::IconMode::Themed {
            let mut resolver = resolver.borrow_mut();
            gui::render_frame(&frame, core.config(), Some(&mut resolver))
        } else {
            gui::render_frame(&frame, core.config(), None)
        }
    };

    rect()
        .expanded()
        .background(gui::WINDOW_CLEAR)
        .on_global_key_down(move |event: Event<KeyboardEventData>| {
            if let Some(action) = gui::key_to_action(&event.key) {
                core.write().apply(action);
                // Enter launched (via the core) or Esc cancelled — the core has
                // stopped. Resident: hide the window and reset so the next toggle
                // shows a clean launcher warm. Otherwise tear the window down.
                if !core.read().running() {
                    if RESIDENT.load(Ordering::SeqCst) {
                        hide_special_workspace();
                        core.write().reset();
                    } else {
                        std::process::exit(0);
                    }
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
            // Wait for the first *presented* frame — the honest time-to-visible —
            // not the much earlier first component render.
            let present_ns = FIRST_PRESENT_NS.load(Ordering::SeqCst);
            if present_ns != 0 {
                // Let the GPU surface settle before we read peak RSS.
                std::thread::sleep(Duration::from_millis(50));
                report(present_ns);
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

/// Print the live native-GUI column: cold-start (to first presented frame) plus
/// footprint, reusing the exact contract the baseline TUI column uses. A
/// render-vs-present breakdown goes to stderr so the gap Skia adds after the
/// first component render is visible. Input-latency / fps come from `--bench`.
fn report(cold_start_ns: u64) {
    let render_ns = FIRST_FRAME_NS.load(Ordering::SeqCst);
    if render_ns != 0 {
        eprintln!(
            "hyprburst-spike-gui: first render {:.3} ms → first present {:.3} ms (Skia/present adds {:.3} ms)",
            render_ns as f64 / 1e6,
            cold_start_ns as f64 / 1e6,
            cold_start_ns.saturating_sub(render_ns) as f64 / 1e6,
        );
    }
    let footprint = bench::live_footprint_for(&["freya-spike"]);
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}
