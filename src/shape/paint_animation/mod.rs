//! Paint-time animation a widget registers per shape, and its vocabulary: the per-shape contract
//! ([`PaintAnimation`] in, [`paint_mod::PaintMod`] out) that `scene::tree::paint_animations`
//! stores per tree.
//!
//! It does not affect layout, hit-test or structure. Unlike `crate::animation`, sampling is a pure
//! function of `now` at encode time, so irregular `dt` does not drift.
//!
//! An animation drives alpha, rotation, or both, and gives an effect its time: the time into the
//! current pass, so it keeps its precision however long the run. Translation and scale would need
//! a swept union the cascade can't bound without sampling; a rotation's cover is the same square at
//! every angle. Refused for an effect's time: one global clock (Godot `TIME`, Shadertoy), which
//! wraps and loses precision; time the app passes (SwiftUI `TimelineView`), which damage cannot see;
//! a motion type of its own, a second clock beside this one.
//!
//! A turn rotates any shape kind about its owner box's center, at paint time only; it is not a
//! rotation in `TranslateScale`, whose axis-aligned rects the scissor and hit-test math rely on.
//! Damage takes the square the shape's bound sweeps, so no angle reaches the cascade. Each kind
//! turns where its geometry is cheapest to turn: a polyline's and a curve's points in the
//! composer, a triangle's in the encoder, a mesh through a complex scale in its instance, a quad
//! and an image through an angle about their own center in the vertex shader (the fragment works
//! in their unturned frame, where a rotation keeps the AA width), and text and icons through a
//! turned instance buffer of their own, so an unturned glyph keeps its size. An effect does not
//! turn: its wrapper computes coverage in the unturned rect, so `Ui::add_shape_animated` refuses a
//! turn on it. Refused:
//! tessellating quads and text into meshes, which loses the SDF's AA and the instanced batching;
//! drawing a turned shape offscreen and compositing it, a pass and a texture per shape.

pub mod curves;
pub(crate) mod paint_mod;

use crate::animation::easing::Easing;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::lerp::Lerp;
use crate::shape::paint_animation::paint_mod::PaintMod;
use std::f32::consts::TAU;
use std::hash;
use std::num::{NonZeroU32, NonZeroU128};
use std::time::Duration;

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// A phase in `[0, 1)` mapped to where the channel sits: `0` at its range's `from`, `1` at its
/// `to`. A value may leave `[0, 1]`, as [`Easing::OutBack`]'s overshoot does: a turn then swings
/// past its range, and an alpha clamps.
///
/// A plain `fn`, so it cannot capture: the encoder samples with no accumulator, so a dropped frame or irregular `dt` must not make it drift.
pub type PaintCurve = fn(f32) -> f32;

/// What an animation's unit value drives. Both channels ride one curve, and a shape carries one
/// animation. With neither, the animation only gives an effect its time ([`PaintAnimation::clock`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintChannel {
    /// Alpha multiplier over its range. `None` leaves opacity alone. The sample is a *fraction*: clamped to `0..=1`, `0` if not finite.
    pub alpha: Option<PaintRange>,
    /// Full turns about the owner box's center over its range. `None` leaves orientation alone; non-finite reads as no turn.
    pub turn: Option<PaintRange>,
}

/// The values a channel moves between as its curve goes from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintRange {
    /// The value at the curve's 0.
    pub from: f32,
    /// The value at the curve's 1.
    pub to: f32,
}

impl PaintRange {
    /// The value at the curve's output `t`.
    fn at(self, t: f32) -> f32 {
        f32::lerp(&self.from, &self.to, t)
    }
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
    /// No pass runs: the end of one pass holds at every time, and an effect reads the period as
    /// its time. What reduced motion makes of a decorative `Once`, as Android's animator duration
    /// scale of 0 ends each animation at once
    /// ([`ANIMATOR_DURATION_SCALE`](https://developer.android.com/reference/android/provider/Settings.Global#ANIMATOR_DURATION_SCALE)).
    Held,
}

/// What an animation's motion tells the person, which decides whether it runs while the system
/// asks for reduced motion. [`Ui::animate`](crate::Ui::animate) snaps every motion then, as a
/// value it moves is the widget's state, never progress.
///
/// WCAG 2.3.3 lets motion that is essential to what the content says stay on under that
/// preference, and asks for the rest to stop
/// ([WCAG 2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintMotion {
    /// Motion that only decorates. Under reduced motion a single pass paints its end value
    /// ([`PaintRepeat::Held`]) and a repeating one paints as if unanimated, as a caret holds still.
    #[default]
    Decorative,
    /// Motion that shows work under way, as a spinner's turn: it runs whatever the preference.
    Progress,
}

/// When an animation runs, and how finely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// # use palantir::Easing;
/// # use palantir::widget::{PaintAnimation, PaintRepeat, curves};
/// # use std::time::Duration;
/// // Fade in over 240 ms, easing out, and stay.
/// let fade = PaintAnimation::alpha(0.0, 1.0)
///     .with_period(Duration::from_millis(240))
///     .with_curve(Easing::OutCubic.curve());
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
    /// Whether it runs under reduced motion; decorative by default.
    pub motion: PaintMotion,
}

impl PaintAnimation {
    /// Feed everything the sampled modifier depends on into `h`. The curve goes in by address: folded addresses mean identical code, and a split costs only a spare repaint.
    pub(crate) fn hash_static(&self, h: &mut impl hash::Hasher) {
        let PaintChannel { alpha, turn } = self.channel;
        for range in [alpha, turn] {
            match range {
                Some(PaintRange { from, to }) => {
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
            PaintRepeat::Held => h.write_u8(3),
        }
        match steps {
            PaintSteps::Continuous => h.write_u32(0),
            PaintSteps::Steps(n) => h.write_u32(n.get()),
        }
        hash::Hash::hash(&self.curve, h);
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
            curve: Easing::Linear.curve(),
            motion: PaintMotion::Decorative,
        }
    }

    /// Animate opacity from `from` to `to`. [`Self::with_alpha`] adds the channel to an existing animation; `turn` and [`Self::with_turn`] pair the same way.
    pub fn alpha(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: Some(PaintRange { from, to }),
            turn: None,
        })
    }

    /// Animate rotation from `from` to `to`, in full turns about the owner box's center.
    pub fn turn(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: None,
            turn: Some(PaintRange { from, to }),
        })
    }

    /// Drive no channel: only give an effect ([`Shape::effect`](crate::widget::Shape::effect)) its
    /// time, the seconds into the current pass. Without an animation an effect's time is 0.
    pub fn clock() -> Self {
        Self::new(PaintChannel {
            alpha: None,
            turn: None,
        })
    }

    /// Add an opacity range to an animation that already turns.
    pub const fn with_alpha(mut self, from: f32, to: f32) -> Self {
        self.channel.alpha = Some(PaintRange { from, to });
        self
    }

    /// Add a rotation range to an animation that already fades.
    pub const fn with_turn(mut self, from: f32, to: f32) -> Self {
        self.channel.turn = Some(PaintRange { from, to });
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

    /// Set the curve, a [`PaintCurve`]; linear by default.
    pub const fn with_curve(mut self, curve: PaintCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Say what the motion tells the person; [`PaintMotion::Decorative`] by default.
    pub const fn with_motion(mut self, motion: PaintMotion) -> Self {
        self.motion = motion;
        self
    }

    /// This animation under reduced motion: `None` paints the shape as if unanimated. A
    /// decorative single pass holds its end at once ([`PaintRepeat::Held`]), its period kept for
    /// an effect's time; a decorative repeat goes; progress runs on.
    pub(crate) const fn reduced(self) -> Option<Self> {
        match (self.motion, self.timing.repeat) {
            (PaintMotion::Progress, _) | (PaintMotion::Decorative, PaintRepeat::Held) => Some(self),
            (PaintMotion::Decorative, PaintRepeat::Once) => Some(Self {
                timing: PaintTiming {
                    started_at: Duration::ZERO,
                    period: self.timing.period,
                    repeat: PaintRepeat::Held,
                    steps: PaintSteps::Continuous,
                },
                ..self
            }),
            (PaintMotion::Decorative, PaintRepeat::Forever | PaintRepeat::Settle(_)) => None,
        }
    }

    /// Whether this drives a channel; one that drives none only gives an effect its time.
    pub(crate) const fn drives_channel(self) -> bool {
        self.channel.alpha.is_some() || self.channel.turn.is_some()
    }

    #[inline]
    pub(crate) fn sample(self, now: Duration) -> PaintMod {
        let Some(phase) = self.timing.phase(now) else {
            return PaintMod::IDENTITY;
        };
        let t = (self.curve)(phase as f32);
        let rotation = |range: PaintRange| {
            let radians = range.at(t) * TAU;
            if radians.is_finite() { radians } else { 0.0 }
        };
        PaintMod {
            alpha: self
                .channel
                .alpha
                .map_or(1.0, |range| domain::fraction(range.at(t))),
            rotation: self.channel.turn.map_or(0.0, rotation),
            time: (phase * self.timing.period.as_secs_f64()) as f32,
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
    /// Where in the curve `now` falls, or `None` once settled. Counted in exact nanoseconds up
    /// to the one division, so a long run reads its pass as precisely as the first.
    #[inline]
    fn phase(self, now: Duration) -> Option<f64> {
        let elapsed = now.saturating_sub(self.started_at);
        match self.repeat {
            PaintRepeat::Held => return Some(1.0),
            PaintRepeat::Settle(after) if elapsed >= after => return None,
            PaintRepeat::Once | PaintRepeat::Forever | PaintRepeat::Settle(_) => {}
        }
        if self.period.is_zero() {
            // Every pass ends at once: a single pass holds its end from the start, a repeat its start.
            let ended = self.repeat == PaintRepeat::Once && now >= self.started_at;
            return Some(if ended { 1.0 } else { 0.0 });
        }
        if let PaintSteps::Steps(n) = self.steps {
            let step = Step::of(self.period, n, elapsed);
            if self.repeat == PaintRepeat::Once && step.pass >= 1 {
                return Some(1.0);
            }
            return Some(step.idx as f64 / f64::from(n.get()));
        }
        let (elapsed, period) = (elapsed.as_nanos(), self.period.as_nanos());
        if self.repeat == PaintRepeat::Once && elapsed >= period {
            return Some(1.0);
        }
        Some((elapsed % period) as f64 / period as f64)
    }

    #[inline]
    const fn settles_at(self) -> Option<Duration> {
        match self.repeat {
            PaintRepeat::Once => Some(self.started_at.saturating_add(self.period)),
            PaintRepeat::Forever => None,
            PaintRepeat::Settle(after) => Some(self.started_at.saturating_add(after)),
            // Settled before any time, whatever its start.
            PaintRepeat::Held => Some(Duration::ZERO),
        }
    }

    #[inline]
    fn next_wake(self, now: Duration) -> Option<Duration> {
        // Checked before the start: every settle but a held pass's falls at or after the start.
        let settles_at = self.settles_at();
        if settles_at.is_some_and(|at| now >= at) {
            return None;
        }
        if now < self.started_at {
            return Some(self.started_at);
        }
        // A window that is not a whole number of steps ends between boundaries, so the settle caps the wake.
        if self.period.is_zero() {
            // Every pass ends instantly; only the settle is left.
            return settles_at;
        }
        let wake = match self.steps {
            PaintSteps::Continuous => now,
            PaintSteps::Steps(n) => {
                let elapsed = now.checked_sub(self.started_at).unwrap();
                let step = Step::of(self.period, n, elapsed);
                let next = step.start_of_next(self.period, n);
                self.started_at.saturating_add(Duration::new(
                    u64::try_from(next / NANOS_PER_SEC).unwrap_or(u64::MAX),
                    u32::try_from(next % NANOS_PER_SEC).unwrap(),
                ))
            }
        };
        Some(settles_at.map_or(wake, |at| wake.min(at)))
    }
}

/// Which of the `n` steps of which pass a time `elapsed` past the start falls in, counted in exact
/// nanoseconds, so the sample and the wake agree on every boundary however long the run. Step
/// `idx` of a pass starts at the first nanosecond whose share of the pass reaches `idx / n`.
#[derive(Clone, Copy, Debug)]
struct Step {
    pass: u128,
    idx: u128,
}

impl Step {
    fn of(period: Duration, n: NonZeroU32, elapsed: Duration) -> Self {
        let period = period.as_nanos();
        let elapsed = elapsed.as_nanos();
        Self {
            pass: elapsed / period,
            idx: elapsed % period * NonZeroU128::from(n).get() / period,
        }
    }

    /// Nanoseconds from the start to the first of the next step, the next pass's first step after
    /// the last one.
    fn start_of_next(self, period: Duration, n: NonZeroU32) -> u128 {
        let period = period.as_nanos();
        let n = NonZeroU128::from(n).get();
        self.pass * period + ((self.idx + 1) * period).div_ceil(n)
    }
}

#[cfg(test)]
mod tests;
