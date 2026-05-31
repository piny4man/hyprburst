//! Phase 5 embedded-terminal POC for the Freya bake-off.
//!
//! A Freya window that hosts a PTY running the **unmodified** `hyprburst tui`
//! (via Freya's `Terminal` component / its `freya-terminal` PTY backend). This
//! variant evaluates owning the terminal host — a GPU/Skia window, Wayland-
//! native, app-id `hyprburst`, styled by the shipped windowrules — while keeping
//! the entire shipped ratatui codepath intact inside the PTY. Keyboard events go
//! straight to the PTY; all launcher behaviour lives in the inner process.
//!
//! Built only with the `freya-spike` feature (see `required-features` in
//! `Cargo.toml`), so the default build and shipped binary never pull Freya.
//!
//! Modes (identical contract to `hyprburst-spike-gui`):
//! - default — open the window and run `hyprburst tui` inside it; the window
//!   closes when the inner process exits. The first-frame metrics print once it
//!   paints.
//! - `--measure` — exit right after the first frame, for unattended cold-start /
//!   peak-RSS capture.
//! - `--bench` — no window: print the baseline vs. native-GUI vs.
//!   embedded-terminal comparison table headlessly, then exit.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use freya::prelude::*;
use freya::terminal::{Terminal, TerminalHandle, TerminalId};
use hyprburst::bench;
use hyprburst::gui;
use hyprburst::term_host;

/// Wayland app-id — must match the id the shipped windowrules target.
const APP_ID: &str = "hyprburst";
/// Column label this variant fills in the comparison table.
const VARIANT: &str = "embedded-term";

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

    let measure = args.iter().any(|a| a == "--measure");

    // Report (and, in --measure, exit) once the first frame lands. Runs off the
    // UI thread so it never blocks Freya's event loop.
    spawn_reporter(measure);

    launch(
        LaunchConfig::new().with_exit_on_close(true).with_window(
            WindowConfig::new(app)
                .with_app_id(APP_ID)
                .with_title("hyprburst (freya spike — embedded terminal)")
                .with_size(f64::from(gui::WINDOW_WIDTH), f64::from(gui::WINDOW_HEIGHT))
                .with_transparency(true)
                .with_background(gui::WINDOW_CLEAR),
        ),
    );
}

/// App root. Spawns the PTY running `hyprburst tui`, stamps the first-frame
/// timestamp on its first render, forwards key events to the PTY, and tears the
/// window down when the inner process exits.
fn app() -> impl IntoElement {
    use_hook(|| {
        if let Some(start) = START.get() {
            let ns = (start.elapsed().as_nanos() as u64).max(1);
            // Only the first stamp counts; ignore if already set.
            let _ = FIRST_FRAME_NS.compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst);
        }
    });

    let handle = use_state(|| {
        TerminalHandle::new(TerminalId::new(), term_host::launcher_command(), None).ok()
    });

    let a11y_id = use_a11y();

    // Close the window when the inner `hyprburst tui` exits (it launches an app
    // via `hyprctl` and stops, or the user presses Esc). Hook is called
    // unconditionally; the None case (PTY failed to start) just never resolves.
    let closer = handle.read().clone();
    use_future(move || {
        let closer = closer.clone();
        async move {
            if let Some(handle) = closer {
                handle.closed().await;
                std::process::exit(0);
            }
        }
    });

    rect()
        .expanded()
        .background(gui::WINDOW_CLEAR)
        .child(match handle.read().clone() {
            Some(handle) => rect()
                .expanded()
                .child(
                    Terminal::new(handle.clone())
                        .a11y_id(a11y_id)
                        .font_family(gui::font_family())
                        .font_size(gui::ENTRY_FONT_SIZE)
                        .background(gui::SURFACE)
                        .foreground(gui::FG)
                        .on_mouse_down(move |_| a11y_id.request_focus())
                        .on_key_down(move |event: Event<KeyboardEventData>| {
                            let _ = handle.write_key(&event.key, event.modifiers);
                        }),
                )
                .into_element(),
            None => "Failed to start the embedded terminal.".into_element(),
        })
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
                    "hyprburst-spike-term: no frame within 10s — is a Wayland display available?"
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

/// Print the live embedded-terminal column: cold-start to first frame plus
/// footprint, reusing the exact contract the baseline TUI column uses. Input-
/// latency / fps for this variant come from the headless `--bench` run.
fn report(cold_start_ns: u64) {
    let footprint = bench::live_footprint_for(&["freya-spike"]);
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}
