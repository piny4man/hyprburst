use std::time::Instant;

use ratatui::prelude::*;
use tachyonfx::{EffectRenderer, Interpolation, Motion, fx};

const FADE_IN_MS: u32 = 220;
const STARTUP_SWEEP_MS: u32 = 260;
const LOADING_COALESCE_MS: u32 = 320;
const QUERY_TRANSITION_MS: u32 = 180;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectPrototype {
    StartupFade,
    StartupSweep,
    LoadingCoalesce,
    QuerySlide,
    QueryDissolve,
    QuerySweep,
}

impl EffectPrototype {
    pub fn name(self) -> &'static str {
        match self {
            Self::StartupFade => "startup-fade",
            Self::StartupSweep => "startup-sweep",
            Self::LoadingCoalesce => "loading-coalesce",
            Self::QuerySlide => "query-slide",
            Self::QueryDissolve => "query-dissolve",
            Self::QuerySweep => "query-sweep",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        all_prototypes()
            .into_iter()
            .find(|prototype| prototype.name() == name)
    }
}

pub fn all_prototypes() -> [EffectPrototype; 6] {
    [
        EffectPrototype::StartupFade,
        EffectPrototype::StartupSweep,
        EffectPrototype::LoadingCoalesce,
        EffectPrototype::QuerySlide,
        EffectPrototype::QueryDissolve,
        EffectPrototype::QuerySweep,
    ]
}

pub fn startup_loading_prototypes() -> [EffectPrototype; 3] {
    [
        EffectPrototype::StartupFade,
        EffectPrototype::StartupSweep,
        EffectPrototype::LoadingCoalesce,
    ]
}

pub fn query_transition_prototypes() -> [EffectPrototype; 3] {
    [
        EffectPrototype::QuerySlide,
        EffectPrototype::QueryDissolve,
        EffectPrototype::QuerySweep,
    ]
}

pub struct PrototypeEffect {
    prototype: EffectPrototype,
    effect: tachyonfx::Effect,
    last_tick: Instant,
}

impl PrototypeEffect {
    pub fn new(prototype: EffectPrototype) -> Self {
        Self {
            prototype,
            effect: build_prototype_effect(prototype),
            last_tick: Instant::now(),
        }
    }

    pub fn prototype(&self) -> EffectPrototype {
        self.prototype
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

fn build_prototype_effect(prototype: EffectPrototype) -> tachyonfx::Effect {
    match prototype {
        EffectPrototype::StartupFade => {
            fx::fade_from_fg(Color::Black, (FADE_IN_MS, Interpolation::QuadOut))
        }
        EffectPrototype::StartupSweep => fx::sweep_in(
            Motion::LeftToRight,
            6,
            0,
            Color::Black,
            (STARTUP_SWEEP_MS, Interpolation::QuadOut),
        ),
        EffectPrototype::LoadingCoalesce => {
            fx::coalesce((LOADING_COALESCE_MS, Interpolation::QuadOut))
        }
        EffectPrototype::QuerySlide => fx::slide_in(
            Motion::RightToLeft,
            4,
            0,
            Color::Black,
            (QUERY_TRANSITION_MS, Interpolation::QuadOut),
        ),
        EffectPrototype::QueryDissolve => {
            fx::dissolve((QUERY_TRANSITION_MS, Interpolation::Linear))
        }
        EffectPrototype::QuerySweep => fx::sweep_in(
            Motion::UpToDown,
            3,
            0,
            Color::Black,
            (QUERY_TRANSITION_MS, Interpolation::QuadOut),
        ),
    }
}

pub struct FadeIn {
    effect: PrototypeEffect,
}

impl FadeIn {
    pub fn new() -> Self {
        Self {
            effect: PrototypeEffect::new(EffectPrototype::StartupFade),
        }
    }

    pub fn is_done(&self) -> bool {
        self.effect.is_done()
    }

    pub fn apply(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.effect.apply(frame, area);
    }

    #[cfg(test)]
    pub fn apply_to_buffer(&mut self, buf: &mut Buffer, area: Rect, elapsed: std::time::Duration) {
        self.effect.apply_to_buffer(buf, area, elapsed);
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

    #[test]
    fn startup_loading_prototypes_offer_multiple_primitives() {
        let prototypes = startup_loading_prototypes();

        assert!(prototypes.contains(&EffectPrototype::StartupFade));
        assert!(prototypes.contains(&EffectPrototype::StartupSweep));
        assert!(prototypes.contains(&EffectPrototype::LoadingCoalesce));
        assert!(prototypes.len() >= 3);
    }

    #[test]
    fn query_transition_prototypes_offer_multiple_primitives() {
        let prototypes = query_transition_prototypes();

        assert!(prototypes.contains(&EffectPrototype::QuerySlide));
        assert!(prototypes.contains(&EffectPrototype::QueryDissolve));
        assert!(prototypes.contains(&EffectPrototype::QuerySweep));
        assert!(prototypes.len() >= 3);
    }

    #[test]
    fn prototype_effects_are_constructed_through_single_wrapper() {
        for prototype in startup_loading_prototypes()
            .into_iter()
            .chain(query_transition_prototypes())
        {
            let effect = PrototypeEffect::new(prototype);
            assert_eq!(effect.prototype(), prototype);
            assert!(!effect.is_done());
        }
    }

    #[test]
    fn prototype_effect_lifecycle_completes_at_wrapper_boundary() {
        let mut effect = PrototypeEffect::new(EffectPrototype::QueryDissolve);
        let area = Rect::new(0, 0, 8, 2);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "Firefox", Style::new().fg(Color::White));

        effect.apply_to_buffer(&mut buf, area, Duration::from_millis(1_000));

        assert!(effect.is_done());
    }

    #[test]
    fn prototype_names_round_trip_for_demo_selection() {
        for prototype in all_prototypes() {
            assert_eq!(
                EffectPrototype::from_name(prototype.name()),
                Some(prototype)
            );
        }

        assert_eq!(
            EffectPrototype::from_name("startup-sweep"),
            Some(EffectPrototype::StartupSweep)
        );
        assert_eq!(EffectPrototype::from_name("missing"), None);
    }
}
