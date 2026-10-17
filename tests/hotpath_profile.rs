//! The opt-in `hotpath` build profiles the open, query, and selection paths.
//!
//! Compiled only with `--features hotpath`: drives a launcher through app
//! discovery, typing, and selection under a profiling guard and checks the
//! report names every phase, so an instrumentation attribute lost in a refactor
//! fails here instead of silently thinning the next profiling run.
#![cfg(feature = "hotpath")]

use hyprburst::domain::config::Config;
use hyprburst::domain::desktop::{DesktopEntry, discover_apps};
use hyprburst::domain::launcher_core::{LauncherAction, LauncherCore};
use hyprburst::view::render::{RenderCache, render_core};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn the_report_names_every_launcher_phase() {
    let report = std::env::temp_dir().join(format!(
        "hyprburst-hotpath-profile-{}.json",
        std::process::id()
    ));
    let guard = hotpath::HotpathGuardBuilder::new("hotpath_profile")
        .format(hotpath::Format::Json)
        .output_path(&report)
        .functions_limit(0)
        .build();

    let _ = discover_apps();
    let apps = ["Firefox", "Files", "Foot", "Steam", "Terminal"]
        .iter()
        .map(|name| DesktopEntry::new(*name, *name, "", name.to_lowercase()))
        .collect();
    let mut core = LauncherCore::from_apps(apps, Config::default());
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let mut cache = RenderCache::new();
    for action in [
        LauncherAction::Insert('f'),
        LauncherAction::Insert('i'),
        LauncherAction::Backspace,
        LauncherAction::MoveDown,
        LauncherAction::MoveUp,
    ] {
        core.apply(action);
        buf.reset();
        render_core(&mut cache, &mut core, area, &mut buf);
    }
    drop(guard);

    let json = std::fs::read_to_string(&report).unwrap();
    let _ = std::fs::remove_file(&report);
    for name in [
        "discover_apps",
        "LauncherCore::apply",
        "filter_and_rank",
        "render_core",
    ] {
        assert!(json.contains(name), "{name} missing from report:\n{json}");
    }
}
