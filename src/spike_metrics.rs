//! Honest first-paint instrumentation for the Freya POCs (bake-off).
//!
//! The POCs originally stamped cold-start from the app root's first *component*
//! render (`use_hook`). On a real session that badly undersells the latency a
//! user feels: Freya creates the window handle and runs the VDOM quickly, but the
//! expensive work — Skia surface realization, first shader compilation, and the
//! actual present to the compositor — happens *after* the first component render.
//! So the metric read "fast" while the window took noticeably longer to appear.
//!
//! [`FirstPaintPlugin`] hooks Freya's [`PluginEvent::AfterPresenting`] — fired
//! "after presenting the canvas to the window" — and stamps the first one. That
//! is the truthful "pixels on screen" timestamp, so the cold-start column now
//! reflects time-to-visible rather than time-to-first-render.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use freya_winit::plugins::{FreyaPlugin, PluginEvent, PluginHandle};

/// Freya plugin that records, once, the elapsed time from a process-start
/// reference to the first frame actually presented to the window.
pub struct FirstPaintPlugin {
    start: Instant,
    stamp: &'static AtomicU64,
}

impl FirstPaintPlugin {
    /// `start` is the process-start reference; `stamp` is set (once, via CAS) to
    /// the nanoseconds elapsed at the first presented frame. `stamp` should begin
    /// at `0` (meaning "not yet painted").
    pub fn new(start: Instant, stamp: &'static AtomicU64) -> Self {
        Self { start, stamp }
    }
}

impl FreyaPlugin for FirstPaintPlugin {
    fn plugin_id(&self) -> &'static str {
        "hyprburst-first-paint"
    }

    fn on_event(&mut self, event: &mut PluginEvent, _handle: PluginHandle) {
        if let PluginEvent::AfterPresenting { .. } = event {
            let ns = (self.start.elapsed().as_nanos() as u64).max(1);
            // Only the first present counts; ignore every later frame.
            let _ = self
                .stamp
                .compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
}

/// Warm-toggle latency for the resident POCs (Phase 7): the elapsed time from a
/// *show-trigger* — the user toggling the hidden window back on — to the first
/// frame presented afterwards. This is the metric that makes a launcher *feel*
/// instant, and the one the spawn-per-launch POCs could never fill.
///
/// Unlike cold-start (a single first-ever paint), warm-toggle repeats: every
/// hide→show cycle arms a fresh trigger, and only the *first* present after that
/// trigger counts. The trigger side ([`arm`](Self::arm)) is driven by the
/// re-show signal — a winit focus/occlusion resume, or `SIGUSR1` in the
/// unattended `--measure` driver — while the present side ([`present`](Self::present),
/// called from [`WarmTogglePlugin`] on each `AfterPresenting`) consumes it and
/// records the latency. Timestamps are passed in (nanoseconds since a fixed
/// process base) rather than read internally, so the state machine is
/// deterministic and unit-testable without a clock or a window.
pub struct WarmToggle {
    /// ns of the pending show-trigger; `0` means none armed.
    trigger_ns: AtomicU64,
    /// ns latency of the most recent completed warm toggle; `0` means none yet.
    latest_ns: AtomicU64,
}

impl WarmToggle {
    /// A fresh capture with no trigger armed and no measurement yet.
    pub const fn new() -> Self {
        Self {
            trigger_ns: AtomicU64::new(0),
            latest_ns: AtomicU64::new(0),
        }
    }

    /// Arm a show-trigger observed at `now_ns`. Replaces any still-pending
    /// trigger so the most recent toggle is the one measured to its next present.
    /// `now_ns` is clamped to ≥1 so `0` stays the "nothing armed" sentinel.
    pub fn arm(&self, now_ns: u64) {
        self.trigger_ns.store(now_ns.max(1), Ordering::SeqCst);
    }

    /// Record a frame presented at `now_ns`. Only the *first* present after an
    /// [`arm`](Self::arm) measures — it consumes the trigger; later presents are
    /// ignored until the next `arm`. Returns the latency when one was recorded.
    pub fn present(&self, now_ns: u64) -> Option<u64> {
        let trigger = self.trigger_ns.load(Ordering::SeqCst);
        if trigger == 0 {
            return None;
        }
        // Consume the trigger; lose the race gracefully if another present did.
        if self
            .trigger_ns
            .compare_exchange(trigger, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return None;
        }
        let latency = now_ns.saturating_sub(trigger).max(1);
        self.latest_ns.store(latency, Ordering::SeqCst);
        Some(latency)
    }

    /// The most recent completed warm-toggle latency in nanoseconds, or `None`
    /// if no toggle has been measured yet.
    pub fn latest(&self) -> Option<u64> {
        match self.latest_ns.load(Ordering::SeqCst) {
            0 => None,
            n => Some(n),
        }
    }
}

impl Default for WarmToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// Freya plugin that stamps warm-toggle latency: on every presented frame it
/// reports the present to a shared [`WarmToggle`], which measures only the first
/// present after a show-trigger was armed. Pairs with the resident binary's
/// re-show handling, which calls [`WarmToggle::arm`] when the window is toggled
/// back on (or on `SIGUSR1` in the unattended driver).
pub struct WarmTogglePlugin {
    start: Instant,
    warm: &'static WarmToggle,
}

impl WarmTogglePlugin {
    /// `start` is the same process-start reference the cold-start plugin uses, so
    /// arm/present timestamps share one base clock.
    pub fn new(start: Instant, warm: &'static WarmToggle) -> Self {
        Self { start, warm }
    }
}

impl FreyaPlugin for WarmTogglePlugin {
    fn plugin_id(&self) -> &'static str {
        "hyprburst-warm-toggle"
    }

    fn on_event(&mut self, event: &mut PluginEvent, _handle: PluginHandle) {
        if let PluginEvent::AfterPresenting { .. } = event {
            let _ = self.warm.present(self.start.elapsed().as_nanos() as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_without_an_armed_trigger_measures_nothing() {
        let warm = WarmToggle::new();
        assert_eq!(warm.present(5_000), None);
        assert_eq!(warm.latest(), None);
    }

    #[test]
    fn first_present_after_arm_measures_and_consumes_the_trigger() {
        let warm = WarmToggle::new();
        warm.arm(1_000);
        // First present after the trigger records latency = present - trigger.
        assert_eq!(warm.present(1_700), Some(700));
        assert_eq!(warm.latest(), Some(700));
        // Later presents don't re-measure until the next arm.
        assert_eq!(warm.present(2_500), None);
        assert_eq!(warm.latest(), Some(700));
    }

    #[test]
    fn re_arming_measures_the_latest_toggle() {
        let warm = WarmToggle::new();
        warm.arm(1_000);
        assert_eq!(warm.present(1_400), Some(400));
        // A second hide→show cycle arms again and measures independently.
        warm.arm(5_000);
        assert_eq!(warm.present(5_900), Some(900));
        assert_eq!(warm.latest(), Some(900));
    }

    #[test]
    fn simultaneous_arm_and_present_never_yields_zero() {
        let warm = WarmToggle::new();
        warm.arm(2_000);
        // A present at the exact trigger instant still reports a floor of 1 ns,
        // so a measured toggle is never mistaken for "nothing recorded".
        assert_eq!(warm.present(2_000), Some(1));
        assert_eq!(warm.latest(), Some(1));
    }
}
