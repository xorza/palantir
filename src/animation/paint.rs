//! Paint-only animation registrations — declarative shape-level
//! animations that don't affect layout, hit-test, or tree structure.
//!
//! Widgets register `PaintAnim` against a freshly-added shape via
//! `Ui::add_shape_animated`. The encoder samples the registered
//! function at paint time and applies the resulting [`PaintMod`] (an
//! alpha multiplier today; transform mod folded in once the renderer
//! can express it) to the per-shape brush. `post_record` folds each
//! anim's `next_wake` into `Ui::repaint_wakes` automatically — widget
//! code never calls `request_repaint_after` for these shapes.
//!
//! Slice 1 ships [`PaintAnim::BlinkOpacity`] only. The rotation /
//! pulse / marquee variants in `docs/roadmap/paint-tick.md` need
//! encoder transform-mod plumbing and land in a follow-up.
//!
//! See `docs/roadmap/paint-tick.md` for the full design.

use std::time::Duration;

/// A paint-time animation contract. Encoded as a small enum so the
/// per-shape registry stays a flat `Vec`; sampling is branch-on-tag
/// rather than a virtual call.
///
/// Sampling is a pure function of `now`. No accumulator state, so
/// dropped frames / irregular `dt` don't drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaintAnim {
    /// Solid for `half_period`, hidden for the next `half_period`,
    /// repeating from `started_at`. The caret-blink shape.
    BlinkOpacity {
        half_period: Duration,
        started_at: Duration,
    },
}

/// Per-shape paint modification sampled from a `PaintAnim`. Encoder
/// folds this into the shape's brush at emit time.
///
/// Today only `alpha` ships; a `transform: TranslateScale` field
/// lands when marquee / rotation variants do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMod {
    /// Multiplies the shape's fill alpha. `1.0` = pass-through;
    /// `0.0` = fully transparent (encoder may drop the emit).
    pub(crate) alpha: f32,
}

impl PaintMod {
    /// Pass-through sample. Returned by `sample_paint_anim` when a
    /// shape has no anim attached, so callers can fold the result
    /// unconditionally.
    pub(crate) const IDENTITY: Self = Self { alpha: 1.0 };

    #[allow(dead_code)] // consumed once Pulse/Marquee land
    #[inline]
    pub(crate) fn is_identity(&self) -> bool {
        self.alpha >= 1.0
    }
}

impl PaintAnim {
    /// Sample the animation at `now`. Pure function — caller is
    /// responsible for clamping `now >= started_at` (this routine
    /// tolerates `now < started_at` by returning the pre-start
    /// phase's value).
    #[inline]
    pub(crate) fn sample(self, now: Duration) -> PaintMod {
        match self {
            PaintAnim::BlinkOpacity {
                half_period,
                started_at,
            } => {
                let alpha = if blink_visible_at(half_period, started_at, now) {
                    1.0
                } else {
                    0.0
                };
                PaintMod { alpha }
            }
        }
    }

    /// Earliest `Duration` (absolute time, same epoch as
    /// `Ui::time` / `started_at`) at which `quantum` will next
    /// change. `post_record` folds the min of every live entry's
    /// `next_wake` into `Ui::repaint_wakes` so widgets don't have to.
    ///
    /// For `BlinkOpacity` this is the next half-period boundary
    /// strictly after `now`.
    #[inline]
    pub(crate) fn next_wake(self, now: Duration) -> Duration {
        match self {
            PaintAnim::BlinkOpacity {
                half_period,
                started_at,
            } => next_blink_boundary(half_period, started_at, now),
        }
    }
}

/// True when a blink with `half_period` starting at `started_at` is
/// in its solid phase at `now`. Pre-start (now < started_at) returns
/// `true` so a freshly-focused caret is immediately visible.
#[inline]
fn blink_visible_at(half_period: Duration, started_at: Duration, now: Duration) -> bool {
    if now <= started_at {
        return true;
    }
    let dt = now - started_at;
    // (dt / half_period) parity: even = solid, odd = hidden.
    let n = duration_div_floor(dt, half_period);
    n & 1 == 0
}

/// Absolute time of the next strictly-future boundary at which the
/// blink flips. Aligns to `started_at + k * half_period` for the
/// smallest `k` with that time `> now`.
#[inline]
fn next_blink_boundary(half_period: Duration, started_at: Duration, now: Duration) -> Duration {
    if half_period.is_zero() {
        return Duration::MAX;
    }
    if now < started_at {
        return started_at;
    }
    let dt = now - started_at;
    let n = duration_div_floor(dt, half_period);
    started_at + half_period.saturating_mul((n + 1) as u32)
}

/// `floor(a / b)` for `Duration`. Returns 0 if `b` is zero.
#[inline]
fn duration_div_floor(a: Duration, b: Duration) -> u64 {
    let bn = b.as_nanos();
    if bn == 0 {
        return 0;
    }
    (a.as_nanos() / bn) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const HP: Duration = Duration::from_millis(500);
    const START: Duration = Duration::from_secs(1);

    #[test]
    fn blink_solid_at_start() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        assert_eq!(a.sample(START).alpha, 1.0);
    }

    #[test]
    fn blink_flips_at_first_boundary() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        // Just before the boundary: still solid.
        let before = START + HP - Duration::from_micros(1);
        assert_eq!(a.sample(before).alpha, 1.0);
        // At the boundary: hidden.
        let at = START + HP;
        assert_eq!(a.sample(at).alpha, 0.0);
        // Two boundaries later: solid again.
        let two = START + HP + HP;
        assert_eq!(a.sample(two).alpha, 1.0);
    }

    #[test]
    fn next_wake_aligns_with_next_boundary() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        // Mid-phase: wake at the next half-period boundary.
        assert_eq!(a.next_wake(START + Duration::from_millis(100)), START + HP,);
        // On the boundary: still wake at the *next* one (strictly
        // future).
        assert_eq!(a.next_wake(START + HP), START + HP + HP);
        // Several periods in.
        assert_eq!(
            a.next_wake(START + HP + HP + Duration::from_millis(50)),
            START + HP + HP + HP,
        );
    }

    #[test]
    fn pre_start_phase_is_solid_and_wakes_at_start() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        let before = START - Duration::from_millis(200);
        assert_eq!(a.sample(before).alpha, 1.0);
        assert_eq!(a.next_wake(before), START);
    }

    #[test]
    fn zero_period_never_wakes() {
        let a = PaintAnim::BlinkOpacity {
            half_period: Duration::ZERO,
            started_at: START,
        };
        // Degenerate, but must not panic. `next_wake` returns MAX so
        // the wake folder treats it as "idle".
        assert_eq!(a.next_wake(START + Duration::from_secs(1)), Duration::MAX);
    }

    #[test]
    fn paint_mod_identity_is_pass_through() {
        assert!(PaintMod::IDENTITY.is_identity());
        assert!(PaintMod { alpha: 1.0 }.is_identity());
        assert!(!PaintMod { alpha: 0.5 }.is_identity());
        assert!(!PaintMod { alpha: 0.0 }.is_identity());
    }
}
