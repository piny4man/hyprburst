//! `HYPRBURST_PERF` end-to-end latency probe.
//!
//! hotpath (see docs/hotpath-profiling.md) explains where time goes inside a
//! frame; this probe measures what the user sees: process start to the first
//! presented frame, and a key press to the first presented frame that shows
//! its effect. With `HYPRBURST_PERF=1` the GPU window prints one stderr line
//! per sample:
//!
//! ```text
//! hyprburst-perf <metric> <frontend> <milliseconds>
//! ```
//!
//! `scripts/perf-summary.sh` turns a log of these into median / p95 / max.
//! Unset (or `0`), the probe does nothing.

use std::ffi::OsStr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::domain::launcher_core::LauncherAction;

/// Which latency target an input counts toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Typing, deleting, or completing the query: re-filters the list.
    Query,
    /// Moving the selection through the filtered list.
    Selection,
}

impl InputKind {
    /// The metric name this input's latency is reported under.
    pub fn metric(self) -> &'static str {
        match self {
            Self::Query => "query-input-to-present",
            Self::Selection => "selection-input-to-present",
        }
    }

    /// Classify a launcher action. Launching, cancelling, and mouse selection
    /// close the window, so they have no presented frame to measure.
    pub fn of_action(action: LauncherAction) -> Option<Self> {
        use LauncherAction::*;
        match action {
            Insert(_) | Backspace | Autocomplete => Some(Self::Query),
            MoveUp | MoveDown | MoveLeft | MoveRight | PageUp | PageDown => Some(Self::Selection),
            SelectEntry(_) | LaunchSelected | Cancel => None,
        }
    }
}

/// Whether a `HYPRBURST_PERF` value turns the probe on: any non-empty value
/// other than `0`.
pub fn env_enabled(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// Whether the probe is on for this process (`HYPRBURST_PERF`, read once).
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_enabled(std::env::var_os("HYPRBURST_PERF").as_deref()))
}

/// Format one sample line.
pub fn line(metric: &str, frontend: &str, elapsed: Duration) -> String {
    format!(
        "hyprburst-perf {metric} {frontend} {:.3}",
        elapsed.as_secs_f64() * 1_000.0
    )
}

/// Print one sample line to stderr.
pub fn emit(metric: &str, frontend: &str, elapsed: Duration) {
    eprintln!("{}", line(metric, frontend, elapsed));
}

/// Tracks the oldest input not yet shown on screen.
///
/// Keys that arrive before the next presented change are folded into the
/// first one, so a burst is measured from its first key — the latency the
/// user waits for.
#[derive(Debug, Default)]
pub struct InputProbe {
    pending: Option<(InputKind, Instant)>,
}

impl InputProbe {
    /// Record an input at `at`.
    pub fn input(&mut self, kind: InputKind, at: Instant) {
        self.pending.get_or_insert((kind, at));
    }

    /// A frame showing a visible change was presented at `at`: return the
    /// pending input and its latency, if any.
    pub fn presented(&mut self, at: Instant) -> Option<(InputKind, Duration)> {
        let (kind, since) = self.pending.take()?;
        Some((kind, at.saturating_duration_since(since)))
    }
}

/// Tracks the first presented frame that carries launcher content.
///
/// The Rio window presents its first frame before the PTY child has drawn
/// anything, so `first-present` alone would time an empty panel. Content is
/// any drawn glyph: the launcher always paints at least its prompt.
#[derive(Debug, Default)]
pub struct OpenProbe {
    done: bool,
}

impl OpenProbe {
    /// A frame was presented `since_start` after process start, drawing
    /// `glyphs` glyphs. Returns the latency the first time content appears.
    pub fn presented(&mut self, glyphs: usize, since_start: Duration) -> Option<Duration> {
        if self.done || glyphs == 0 {
            return None;
        }
        self.done = true;
        Some(since_start)
    }

    /// Whether content has been presented yet.
    pub fn done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_content_skips_empty_frames_and_fires_once() {
        let mut probe = OpenProbe::default();
        assert_eq!(probe.presented(0, Duration::from_millis(80)), None);
        assert!(!probe.done());
        assert_eq!(
            probe.presented(12, Duration::from_millis(95)),
            Some(Duration::from_millis(95))
        );
        assert!(probe.done());
        assert_eq!(probe.presented(12, Duration::from_millis(110)), None);
    }

    #[test]
    fn env_value_enables_unless_empty_or_zero() {
        assert!(!env_enabled(None));
        assert!(!env_enabled(Some(OsStr::new(""))));
        assert!(!env_enabled(Some(OsStr::new("0"))));
        assert!(env_enabled(Some(OsStr::new("1"))));
        assert!(env_enabled(Some(OsStr::new("yes"))));
    }

    #[test]
    fn actions_split_into_query_and_selection() {
        use LauncherAction::*;
        for action in [Insert('a'), Backspace, Autocomplete] {
            assert_eq!(InputKind::of_action(action), Some(InputKind::Query));
        }
        for action in [MoveUp, MoveDown, MoveLeft, MoveRight, PageUp, PageDown] {
            assert_eq!(InputKind::of_action(action), Some(InputKind::Selection));
        }
        for action in [LaunchSelected, Cancel, SelectEntry(0)] {
            assert_eq!(InputKind::of_action(action), None);
        }
    }

    #[test]
    fn line_is_stable() {
        assert_eq!(
            line(
                InputKind::Query.metric(),
                "rio",
                Duration::from_micros(12_345)
            ),
            "hyprburst-perf query-input-to-present rio 12.345"
        );
        assert_eq!(InputKind::Selection.metric(), "selection-input-to-present");
    }

    #[test]
    fn a_burst_is_measured_from_its_first_key() {
        let t0 = Instant::now();
        let mut probe = InputProbe::default();
        assert_eq!(probe.presented(t0), None);

        probe.input(InputKind::Query, t0);
        probe.input(InputKind::Selection, t0 + Duration::from_millis(5));
        assert_eq!(
            probe.presented(t0 + Duration::from_millis(20)),
            Some((InputKind::Query, Duration::from_millis(20)))
        );
        // Consumed: a later frame with no new input reports nothing.
        assert_eq!(probe.presented(t0 + Duration::from_millis(40)), None);
    }
}
