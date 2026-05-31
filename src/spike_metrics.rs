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
