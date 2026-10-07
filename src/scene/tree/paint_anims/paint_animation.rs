//! Paint-time animation a widget registers, and its vocabulary.

use crate::animation::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_mod::PaintMod;
use std::f32::consts::TAU;
use std::hash;
use std::num::NonZeroU32;
use std::time::Duration;

/// A phase in `[0, 1)` mapped to a unit value in `[0, 1]`.
///
/// A plain `fn`, so it cannot capture: the encoder samples with no accumulator, so a dropped frame or irregular `dt` must not make it drift.
pub type PaintCurve = fn(f32) -> f32;

/// What an animation's unit value drives. Both channels ride one curve, and a shape carries one animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintChannel {
    /// Alpha multiplier, lerped `from` → `to`. `None` leaves opacity alone. The sample is a *fraction*: clamped to `0..=1`, `0` if not finite.
    pub alpha: Option<(f32, f32)>,
    /// Full turns about the owner box's centre, lerped `from` → `to`. `None` leaves orientation alone; non-finite reads as no turn. Honoured on polylines, curves and arcs only.
    pub turn: Option<(f32, f32)>,
}

/// How often the curve is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintSteps {
    /// Read at every frame's exact phase.
    Continuous,
    /// Hold one value per `1/n` of the period and wake only on boundaries, so a blinking caret asks for no identical frames. Non-zero by type.
    Steps(NonZeroU32),
}

/// What happens after one pass of the curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintRepeat {
    /// One pass, then hold the end value.
    Once,
    /// Repeat while the shape is recorded.
    Forever,
    /// Repeat until this much has elapsed, then paint as if unanimated.
    ///
    /// Evaluated here because a paint-only wake runs no widget code: a record-time cutoff would never fire and the blink would run forever.
    Settle(Duration),
}

/// When an animation runs, and how finely.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintTiming {
    /// Absolute time the first pass begins, in the frame clock's epoch.
    pub started_at: Duration,
    /// One pass of the curve. A zero period finishes each pass instantly: `Once` holds the end value, a repeat the start value.
    pub period: Duration,
    /// How many passes run, and whether they settle.
    pub repeat: PaintRepeat,
    /// Continuous, or quantized.
    pub steps: PaintSteps,
}

/// A paint-time animation: what it drives, when it runs, and the curve between.
///
/// Sampled by the encoder, so the recorded subtree is byte-identical every frame and its layout cache entry survives, unlike [`Ui::animate`](crate::Ui::animate) with [`Ui::request_repaint`](crate::Ui::request_repaint).
///
/// Hand one to [`Ui::add_shape_animated`](crate::Ui::add_shape_animated).
/// ```
/// # use palantir::widget::{PaintAnimation, PaintRepeat, curves};
/// # use std::time::Duration;
/// // Fade in over 240 ms and stay.
/// let fade = PaintAnimation::alpha(0.0, 1.0)
///     .with_period(Duration::from_millis(240))
///     .with_curve(curves::linear);
///
/// // Breathe, forever.
/// let pulse = PaintAnimation::alpha(0.4, 1.0)
///     .with_period(Duration::from_secs(2))
///     .with_repeat(PaintRepeat::Forever)
///     .with_curve(curves::sine);
/// ```
///
/// No `PartialEq`: curves compare by address. Compare [`Self::channel`] and [`Self::timing`].
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct PaintAnimation {
    /// What the animation drives.
    pub channel: PaintChannel,
    /// When it runs, and how finely.
    pub timing: PaintTiming,
    /// Phase in, value out.
    pub curve: PaintCurve,
}

impl PaintAnimation {
    /// Feed everything the sampled modifier depends on into `h`. The curve goes in by address: folded addresses mean identical code, and a split costs only a spare repaint.
    pub(crate) fn hash_static(&self, h: &mut impl hash::Hasher) {
        let PaintChannel { alpha, turn } = self.channel;
        for range in [alpha, turn] {
            match range {
                Some((from, to)) => {
                    h.write_u8(1);
                    from.hash_eq(h);
                    to.hash_eq(h);
                }
                None => h.write_u8(0),
            }
        }
        let PaintTiming {
            started_at,
            period,
            repeat,
            steps,
        } = self.timing;
        h.write_u128(started_at.as_nanos());
        h.write_u128(period.as_nanos());
        match repeat {
            PaintRepeat::Once => h.write_u8(0),
            PaintRepeat::Forever => h.write_u8(1),
            PaintRepeat::Settle(after) => {
                h.write_u8(2);
                h.write_u128(after.as_nanos());
            }
        }
        match steps {
            PaintSteps::Continuous => h.write_u32(0),
            PaintSteps::Steps(n) => h.write_u32(n.get()),
        }
        h.write_usize(self.curve as usize);
    }

    fn new(channel: PaintChannel) -> Self {
        Self {
            channel,
            timing: PaintTiming {
                started_at: Duration::ZERO,
                period: Duration::from_secs(1),
                repeat: PaintRepeat::Once,
                steps: PaintSteps::Continuous,
            },
            curve: curves::linear,
        }
    }

    /// Animate opacity from `from` to `to`. [`Self::with_alpha`] adds the channel to an existing animation; `turn` and [`Self::with_turn`] pair the same way.
    pub fn alpha(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: Some((from, to)),
            turn: None,
        })
    }

    /// Animate rotation from `from` to `to`, in full turns about the owner box's centre.
    pub fn turn(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: None,
            turn: Some((from, to)),
        })
    }

    /// Add an opacity range to an animation that already turns.
    pub const fn with_alpha(mut self, from: f32, to: f32) -> Self {
        self.channel.alpha = Some((from, to));
        self
    }

    /// Add a rotation range to an animation that already fades.
    pub const fn with_turn(mut self, from: f32, to: f32) -> Self {
        self.channel.turn = Some((from, to));
        self
    }

    /// Length of one pass. Default one second.
    pub const fn with_period(mut self, period: Duration) -> Self {
        self.timing.period = period;
        self
    }

    /// Begin at this absolute time; before it the phase is zero.
    pub const fn with_started_at(mut self, at: Duration) -> Self {
        self.timing.started_at = at;
        self
    }

    /// How many passes run. Default one.
    pub const fn with_repeat(mut self, repeat: PaintRepeat) -> Self {
        self.timing.repeat = repeat;
        self
    }

    /// Read the curve at `n` evenly spaced phases and wake only on those boundaries.
    ///
    /// # Panics
    ///
    /// Panics on zero steps.
    #[track_caller]
    pub const fn with_steps(mut self, n: u32) -> Self {
        let n = NonZeroU32::new(domain::count(n)).expect("a count is non-zero");
        self.timing.steps = PaintSteps::Steps(n);
        self
    }

    /// Set the curve; any `fn(f32) -> f32` over `0.0..=1.0`.
    pub const fn with_curve(mut self, curve: PaintCurve) -> Self {
        self.curve = curve;
        self
    }

    #[inline]
    pub(crate) fn sample(self, now: Duration) -> PaintMod {
        let Some(phase) = self.timing.phase(now) else {
            return PaintMod::IDENTITY;
        };
        let t = (self.curve)(phase);
        let rotation = |(a, b)| {
            let radians = f32::lerp(a, b, t) * TAU;
            if radians.is_finite() { radians } else { 0.0 }
        };
        PaintMod {
            alpha: self
                .channel
                .alpha
                .map_or(1.0, |(a, b)| domain::fraction(f32::lerp(a, b, t))),
            rotation: self.channel.turn.map_or(0.0, rotation),
        }
    }

    /// Whether this turns the shape, so damage is the swept square rather than the recorded bbox. A constant turn counts.
    #[inline]
    pub(crate) const fn rotates(self) -> bool {
        self.channel.turn.is_some()
    }

    /// Earliest absolute time the sample changes, or `None` if never again.
    #[inline]
    pub(crate) fn next_wake(self, now: Duration) -> Option<Duration> {
        self.timing.next_wake(now)
    }
}

impl PaintTiming {
    /// Where in the curve `now` falls, or `None` once settled.
    #[inline]
    fn phase(self, now: Duration) -> Option<f32> {
        let elapsed = now.saturating_sub(self.started_at);
        if let PaintRepeat::Settle(after) = self.repeat
            && elapsed >= after
        {
            return None;
        }
        let passes = if self.period.is_zero() {
            if now >= self.started_at { 1.0 } else { 0.0 }
        } else {
            elapsed.as_secs_f64() / self.period.as_secs_f64()
        };
        let raw = match self.repeat {
            PaintRepeat::Once => passes.min(1.0),
            PaintRepeat::Forever | PaintRepeat::Settle(_) => passes.fract(),
        };
        Some(self.quantize(raw as f32))
    }

    #[inline]
    fn quantize(self, phase: f32) -> f32 {
        match self.steps {
            PaintSteps::Continuous => phase,
            PaintSteps::Steps(n) => {
                let n = n.get() as f32;
                (phase * n).floor() / n
            }
        }
    }

    #[inline]
    const fn settles_at(self) -> Option<Duration> {
        match self.repeat {
            PaintRepeat::Once => Some(self.started_at.saturating_add(self.period)),
            PaintRepeat::Forever => None,
            PaintRepeat::Settle(after) => Some(self.started_at.saturating_add(after)),
        }
    }

    #[inline]
    fn next_wake(self, now: Duration) -> Option<Duration> {
        if now < self.started_at {
            return Some(self.started_at);
        }
        // A window that is not a whole number of steps ends between boundaries, so the settle caps the wake.
        let settles_at = self.settles_at();
        if settles_at.is_some_and(|at| now >= at) {
            return None;
        }
        if self.period.is_zero() {
            // Every pass ends instantly; only the settle is left.
            return settles_at;
        }
        let wake = match self.steps {
            PaintSteps::Continuous => now,
            PaintSteps::Steps(n) => {
                let step = self.period / n.get();
                if step.is_zero() {
                    return None;
                }
                let elapsed = now.checked_sub(self.started_at).unwrap();
                let k = (elapsed.as_nanos() / step.as_nanos()) as u32;
                self.started_at + step.saturating_mul(k + 1)
            }
        };
        Some(settles_at.map_or(wake, |at| wake.min(at)))
    }
}
