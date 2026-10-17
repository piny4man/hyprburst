//! Offscreen query/selection cost probe: drives the launcher through typing,
//! deleting, and moving the selection, rendering each step into a buffer, and
//! prints per-action timings — no window or compositor needed.
//!
//! The live input-to-present latency is measured with `HYPRBURST_PERF=1` (see
//! `docs/hotpath-profiling.md`); this is the quick loop for comparing two
//! builds or power profiles on identical input:
//!
//! ```text
//! cargo run --release --example query-cost -- [--live] [rounds] [idle-ms]
//! ```
//!
//! By default it uses a fixed synthetic app set so runs compare across
//! machines; `--live` uses this machine's discovered apps instead. A tight
//! loop keeps the core clocked up, which flatters the balanced profile; pass
//! `idle-ms` (e.g. `200`) to sleep between steps, as a person typing does.
//!
//! Add `--features hotpath` (or `hotpath-alloc`) for a per-function timing (or
//! allocation) breakdown when it exits.

use std::time::{Duration, Instant};

use hyprburst::domain::config::Config;
use hyprburst::domain::desktop::{DesktopEntry, discover_apps};
use hyprburst::domain::launcher_core::{LauncherAction, LauncherCore};
use hyprburst::view::render::{RenderCache, render_core};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Rounds of the scripted sequence by default.
const ROUNDS: usize = 40;
/// Synthetic app count, roughly a full desktop's worth of entries.
const SYNTHETIC_APPS: usize = 300;

const WORDS: &[&str] = &[
    "Fire", "Files", "Term", "Code", "Music", "Photo", "Mail", "Chat", "Steam", "Video", "Note",
    "Paint", "Calc", "Disk", "Print",
];

fn synthetic_apps() -> Vec<DesktopEntry> {
    (0..SYNTHETIC_APPS)
        .map(|i| {
            let name = format!(
                "{} {} {i}",
                WORDS[i % WORDS.len()],
                WORDS[(i / WORDS.len()) % WORDS.len()]
            );
            let id = format!("app{i}.desktop");
            DesktopEntry::new(id, name, "", format!("app{i}"))
        })
        .collect()
}

fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    sorted[(sorted.len() * pct / 100).min(sorted.len() - 1)]
}

fn report(name: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    println!(
        "{name:<10} n={:<4} median={:>7.3}ms p95={:>7.3}ms max={:>7.3}ms",
        samples.len(),
        percentile(samples, 50).as_secs_f64() * 1e3,
        percentile(samples, 95).as_secs_f64() * 1e3,
        samples[samples.len() - 1].as_secs_f64() * 1e3,
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    let live = args.next_if(|a| a == "--live").is_some();
    let rounds = args.next().map_or(Ok(ROUNDS), |s| s.parse())?;
    let idle = Duration::from_millis(args.next().map_or(Ok(0), |s| s.parse())?);

    let _profile = hotpath::HotpathGuardBuilder::new("query-cost")
        .percentiles(&[50.0, 95.0, 99.0])
        .build();

    let started = Instant::now();
    let apps = if live {
        discover_apps()
    } else {
        synthetic_apps()
    };
    let mut core = LauncherCore::from_apps(apps, Config::default());
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let mut cache = RenderCache::new();
    render_core(&mut cache, &mut core, area, &mut buf);
    println!(
        "open       {:.3}ms ({} apps)",
        started.elapsed().as_secs_f64() * 1e3,
        core.view().entries.len()
    );

    let mut query = Vec::new();
    let mut selection = Vec::new();
    for _ in 0..rounds {
        let typing = "fi".chars().map(LauncherAction::Insert);
        let deleting = [LauncherAction::Backspace; 2];
        let moving = [
            LauncherAction::MoveDown,
            LauncherAction::MoveDown,
            LauncherAction::PageDown,
            LauncherAction::MoveUp,
        ];
        for action in typing.chain(moving).chain(deleting) {
            if !idle.is_zero() {
                std::thread::sleep(idle);
            }
            let step = Instant::now();
            core.apply(action);
            buf.reset();
            render_core(&mut cache, &mut core, area, &mut buf);
            let elapsed = step.elapsed();
            match action {
                LauncherAction::Insert(_) | LauncherAction::Backspace => query.push(elapsed),
                _ => selection.push(elapsed),
            }
        }
    }
    report("query", &mut query);
    report("selection", &mut selection);
    Ok(())
}
