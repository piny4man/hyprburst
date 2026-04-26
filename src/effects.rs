use std::time::Instant;

use ratatui::prelude::*;
use tachyonfx::{EffectRenderer, Interpolation, fx};

const FADE_IN_MS: u32 = 220;

pub struct FadeIn {
    effect: tachyonfx::Effect,
    last_tick: Instant,
}

impl FadeIn {
    pub fn new() -> Self {
        Self {
            effect: fx::fade_from_fg(Color::Black, (FADE_IN_MS, Interpolation::QuadOut)),
            last_tick: Instant::now(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.effect.done()
    }

    pub fn apply(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        frame.render_effect(&mut self.effect, area, elapsed.into());
    }

    #[cfg(test)]
    pub fn apply_to_buffer(&mut self, buf: &mut Buffer, area: Rect, elapsed: std::time::Duration) {
        self.last_tick = Instant::now();
        buf.render_effect(&mut self.effect, area, elapsed.into());
    }
}

impl Default for FadeIn {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn fade_starts_not_done() {
        let fade = FadeIn::new();
        assert!(!fade.is_done());
    }

    #[test]
    fn fade_completes_after_sufficient_elapsed_time() {
        let mut fade = FadeIn::new();
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        // Advance well past the fade-in duration in a single tick.
        fade.apply_to_buffer(&mut buf, area, Duration::from_millis(1_000));
        assert!(fade.is_done());
    }

    #[test]
    fn fade_stays_running_before_duration_elapses() {
        let mut fade = FadeIn::new();
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        fade.apply_to_buffer(&mut buf, area, Duration::from_millis(16));
        assert!(!fade.is_done());
    }

    #[test]
    fn fade_darkens_foreground_at_start() {
        let mut fade = FadeIn::new();
        let area = Rect::new(0, 0, 5, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "hello", Style::new().fg(Color::White));
        // Zero elapsed: effect is at t=0, fg should be the "from" color (black).
        fade.apply_to_buffer(&mut buf, area, Duration::ZERO);
        assert_eq!(buf[(0, 0)].fg, Color::Black);
    }

    #[test]
    fn fade_reaches_target_foreground_when_complete() {
        let mut fade = FadeIn::new();
        let area = Rect::new(0, 0, 5, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "hello", Style::new().fg(Color::White));
        fade.apply_to_buffer(&mut buf, area, Duration::from_millis(1_000));
        assert_eq!(buf[(0, 0)].fg, Color::White);
    }
}
