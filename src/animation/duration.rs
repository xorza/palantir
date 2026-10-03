//! Duration motion model: the authored `(secs, ease)` curve's validity
//! bound and its snap-if-close floor.
//!
//! Sibling of [`spring`](crate::animation::spring) — each motion model
//! owns the predicate that admits its parameters and the tolerance its
//! rows settle against, so neither file has to explain the other's
//! numbers.

use crate::animation::animatable::Animatable;

const MAX_DURATION_SECS: f32 = 60.0;

pub(super) const DURATION_ERROR: &str =
    "animation duration must be finite and in 0.0..=60.0 seconds";

/// Whether `secs` names a duration this crate will animate over.
pub(super) const fn duration_is_valid(secs: f32) -> bool {
    secs.is_finite() && secs >= 0.0 && secs <= MAX_DURATION_SECS
}

/// `displacement` is under the type's settle tolerance — the caller can
/// snap to target without animating, because the target barely moved.
/// Consumed by the duration arm of the snap-if-close fast path in
/// `AnimMapTyped::tick`.
///
/// The spring's tolerance, per [`Animatable::settle_distance_squared`]: a
/// motion should run its full designed curve for *any* visible target
/// change, and snap without animating only when the target moved by
/// sub-perceptual drift (ulp rounding in upstream theme math). Duration
/// rows carry no velocity, so this is a position-only check, run on a
/// retarget alone; curve completion is `MotionRow::advance`'s
/// `progress >= 1` arm.
#[inline]
pub(super) fn within_duration_snap_eps<T: Animatable>(displacement: T) -> bool {
    displacement.settle_distance_squared() < 1.0
}
