//! The paint-time animation a widget registers, and the vocabulary it is
//! written in.

use crate::animation::animatable::Animatable;
use crate::scene::tree::paint_anims::PaintMod;
use crate::scene::tree::paint_anims::curves;
use std::f32::consts::TAU;
use std::time::Duration;

/// A phase in `[0, 1)` mapped to a unit value in `[0, 1]`.
///
/// A plain `fn`, so an animation stays `Copy`, allocates nothing, and
/// **cannot capture** — which is what makes it pure without asking anyone
/// to promise. Purity is load-bearing: the encoder samples at paint time
/// with no accumulator, so a dropped frame or an irregular `dt` must not
/// make the animation drift.
///
/// The crate ships [`curves`](crate::curves); anything else is a function
/// the caller writes.
pub type PaintCurve = fn(f32) -> f32;

/// What an animation's unit value drives.
///
/// Both channels ride one curve, so a shape that fades while it turns is
/// one animation. A shape carries one animation, so this is also the only
/// way to drive two channels at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintChannel {
    /// Alpha multiplier, lerped `from` → `to` by the curve. `None`
    /// leaves the shape's own opacity alone.
    pub alpha: Option<(f32, f32)>,
    /// Turns about the owner box's centre, in **full turns**, lerped
    /// `from` → `to`. `None` leaves the shape's orientation alone.
    ///
    /// Honoured on stroked shapes — polylines, curves and arcs. A quad,
    /// a text run and an image cannot be turned.
    pub turn: Option<(f32, f32)>,
}

/// How often the curve is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintSteps {
    /// Read at every frame's exact phase, so the shape repaints as fast
    /// as the host runs.
    Continuous,
    /// Hold one value per `1/n` of the period, and wake only on the
    /// boundaries.
    ///
    /// **What keeps a blinking caret off the frame budget.** A square
    /// curve at `Steps(2)` changes twice a period, so asking for a frame
    /// in between would buy an identical picture.
    Steps(u32),
}

/// What happens after one pass of the curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintRepeat {
    /// One pass, then hold the curve's end value. A fade-in that stays
    /// faded in.
    Once,
    /// Repeat for as long as the shape is recorded. A spinner.
    Forever,
    /// Repeat until this much has elapsed, then stop modifying the shape
    /// at all — not hold the end value, but paint as if unanimated.
    ///
    /// **The idle cutoff, and it has to live here.** A blinking caret is
    /// enough on its own to wake the host, and such a wake paints without
    /// a record pass — so no widget code runs to re-decide anything. A
    /// cutoff evaluated at record time is evaluated once and never again,
    /// and the blink runs forever. Evaluated here, it settles on the
    /// paint pass that crosses it, and the framework stops asking for
    /// frames on its own.
    Settle(Duration),
}

/// When an animation runs, and how finely.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintTiming {
    /// Absolute time the first pass begins, in the frame clock's epoch.
    /// Before it the animation reads at phase zero.
    pub started_at: Duration,
    /// One pass of the curve.
    pub period: Duration,
    pub repeat: PaintRepeat,
    pub steps: PaintSteps,
}

/// A paint-time animation: what it drives, when it runs, and the curve
/// between.
///
/// Sampled by the **encoder**, one pass after record, so the recorded
/// subtree is byte-identical every frame — the widget never re-records
/// and its layout cache entry survives. That is what this buys over
/// [`Ui::animate`](crate::Ui::animate) plus
/// [`Ui::request_repaint`](crate::Ui::request_repaint), which produce the
/// same pixels at the cost of a record pass per frame.
///
/// **The framework owns the schedule; the caller owns the curve.** Which
/// region to damage and when to wake are answered from
/// [`PaintChannel`] and [`PaintTiming`] without calling the curve, so a
/// curve that misbehaves paints a wrong picture rather than painting
/// outside the region cleared for it.
///
/// Hand one to [`Ui::add_shape_animated`](crate::Ui::add_shape_animated).
///
/// ```
/// # use palantir::{PaintAnim, PaintRepeat, curves};
/// # use std::time::Duration;
/// // Fade in over 240 ms and stay.
/// let fade = PaintAnim::alpha(0.0, 1.0)
///     .period(Duration::from_millis(240))
///     .curve(curves::linear);
///
/// // Breathe, forever.
/// let pulse = PaintAnim::alpha(0.4, 1.0)
///     .period(Duration::from_secs(2))
///     .repeat(PaintRepeat::Forever)
///     .curve(curves::sine);
/// ```
///
/// No `PartialEq`: two animations agree when their channel and timing do,
/// and comparing the curves would compare function addresses, which say
/// nothing. Compare [`Self::channel`] and [`Self::timing`] instead.
#[derive(Clone, Copy, Debug)]
pub struct PaintAnim {
    pub channel: PaintChannel,
    pub timing: PaintTiming,
    pub curve: PaintCurve,
}

impl PaintAnim {
    /// One pass of [`curves::linear`](crate::curves::linear) over a
    /// one-second period, driving nothing. The builders below name a
    /// channel and adjust the timing.
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

    /// Animate opacity from `from` to `to`.
    pub fn alpha(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: Some((from, to)),
            turn: None,
        })
    }

    /// Animate rotation from `from` to `to`, in full turns about the
    /// owner box's centre.
    pub fn turn(from: f32, to: f32) -> Self {
        Self::new(PaintChannel {
            alpha: None,
            turn: Some((from, to)),
        })
    }

    /// Add an opacity range to an animation that already turns, so one
    /// curve drives both.
    pub fn with_alpha(mut self, from: f32, to: f32) -> Self {
        self.channel.alpha = Some((from, to));
        self
    }

    /// Add a rotation range to an animation that already fades.
    pub fn with_turn(mut self, from: f32, to: f32) -> Self {
        self.channel.turn = Some((from, to));
        self
    }

    /// One pass of the curve takes this long. One second by default.
    pub fn period(mut self, period: Duration) -> Self {
        self.timing.period = period;
        self
    }

    /// Begin at this absolute time rather than the clock's origin.
    /// Before it the animation reads at phase zero.
    pub fn started_at(mut self, at: Duration) -> Self {
        self.timing.started_at = at;
        self
    }

    pub fn repeat(mut self, repeat: PaintRepeat) -> Self {
        self.timing.repeat = repeat;
        self
    }

    /// Read the curve at `n` evenly spaced phases instead of
    /// continuously, and wake only on those boundaries.
    ///
    /// # Panics
    ///
    /// Panics on zero steps. It would read as a shape that never
    /// animates, with no other sign that the animation was asked for —
    /// and this is a cold builder, so the check costs a frame nothing.
    pub fn steps(mut self, n: u32) -> Self {
        assert!(n > 0, "a paint animation cannot have zero steps");
        self.timing.steps = PaintSteps::Steps(n);
        self
    }

    pub fn curve(mut self, curve: PaintCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Sample at `now`. Pure — `now` is the only input, so a dropped
    /// frame changes nothing about what the next one paints.
    #[inline]
    pub(crate) fn sample(self, now: Duration) -> PaintMod {
        let Some(phase) = self.timing.phase(now) else {
            return PaintMod::IDENTITY;
        };
        let t = (self.curve)(phase);
        PaintMod {
            alpha: self.channel.alpha.map_or(1.0, |(a, b)| f32::lerp(a, b, t)),
            rotation: self
                .channel
                .turn
                .map_or(0.0, |(a, b)| f32::lerp(a, b, t) * TAU),
        }
    }

    /// Whether this turns the shape's geometry, which decides whether the
    /// damage bound is the recorded bbox or the square it sweeps.
    ///
    /// Answered from the channel alone, with no `now` and no call into
    /// the curve: the swept square is the cover at every angle, so the
    /// cascade never needs the value the encoder samples one pass later.
    /// A constant turn counts — the recorded bbox is unrotated either
    /// way.
    #[inline]
    pub(crate) fn rotates(self) -> bool {
        self.channel.turn.is_some()
    }

    /// Earliest absolute time at which the sample changes, or `None`
    /// once it never will again. `post_record` folds the minimum over
    /// every live entry into the wake queue, so a widget never schedules
    /// its own repaint for one of these.
    #[inline]
    pub(crate) fn next_wake(self, now: Duration) -> Option<Duration> {
        self.timing.next_wake(now)
    }
}

impl PaintTiming {
    /// Where in the curve `now` falls, or `None` once the animation has
    /// settled and stopped modifying the shape.
    #[inline]
    fn phase(self, now: Duration) -> Option<f32> {
        if self.period.is_zero() {
            return Some(0.0);
        }
        let elapsed = now.saturating_sub(self.started_at);
        let raw = match self.repeat {
            PaintRepeat::Once => (elapsed.as_secs_f64() / self.period.as_secs_f64()).min(1.0),
            PaintRepeat::Forever => (elapsed.as_secs_f64() / self.period.as_secs_f64()).fract(),
            PaintRepeat::Settle(after) => {
                if elapsed >= after {
                    return None;
                }
                (elapsed.as_secs_f64() / self.period.as_secs_f64()).fract()
            }
        };
        Some(self.quantize(raw as f32))
    }

    /// Snap a phase to the start of its step. `Continuous` reads it as
    /// it is.
    #[inline]
    fn quantize(self, phase: f32) -> f32 {
        match self.steps {
            PaintSteps::Continuous => phase,
            PaintSteps::Steps(0) => 0.0,
            PaintSteps::Steps(n) => (phase * n as f32).floor() / n as f32,
        }
    }

    /// The absolute time this animation stops changing, or `None` when it
    /// never does.
    #[inline]
    fn settles_at(self) -> Option<Duration> {
        match self.repeat {
            PaintRepeat::Once => Some(self.started_at.saturating_add(self.period)),
            PaintRepeat::Forever => None,
            PaintRepeat::Settle(after) => Some(self.started_at.saturating_add(after)),
        }
    }

    #[inline]
    fn next_wake(self, now: Duration) -> Option<Duration> {
        if self.period.is_zero() {
            return None;
        }
        if now < self.started_at {
            return Some(self.started_at);
        }
        // The settle is itself a change the encoder has to paint, so it
        // caps the wake rather than merely bounding it: a window that is
        // not a whole number of steps lands between two boundaries, and
        // waking only on boundaries would leave the shape stuck on
        // whatever phase it was in until some unrelated wake arrived.
        let settles_at = self.settles_at();
        if settles_at.is_some_and(|at| now >= at) {
            return None;
        }
        let wake = match self.steps {
            PaintSteps::Continuous => now,
            PaintSteps::Steps(0) => return None,
            PaintSteps::Steps(n) => {
                let step = self.period / n;
                if step.is_zero() {
                    return None;
                }
                let elapsed = now - self.started_at;
                let k = (elapsed.as_nanos() / step.as_nanos()) as u32;
                self.started_at + step.saturating_mul(k + 1)
            }
        };
        Some(settles_at.map_or(wake, |at| wake.min(at)))
    }
}
