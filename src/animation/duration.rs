//! Duration motion model: the authored `(secs, ease)` curve's validity
//! bound and its snap-if-close floor.
//!
//! Sibling of [`spring`](crate::animation::spring) — each motion model
//! owns the predicate that admits its parameters and the tolerance its
//! rows settle against, so neither file has to explain the other's
//! numbers.

use crate::animation::animatable::Animatable;
use crate::primitives::approx::EPS;

const MAX_DURATION_SECS: f32 = 60.0;

pub(super) const DURATION_ERROR: &str =
    "animation duration must be finite and in 0.0..=60.0 seconds";

// Duration snap-if-close floor. Far tighter than the spring floor
// (`spring::POS_EPS`): a duration animation should run its full
// designed curve for *any* visible target change, and
// snap-without-animating only when the target moved by sub-perceptual
// drift (ulp rounding in upstream theme math). The spring floor is
// pixel-scale-loose; reusing it here made sub-1% colour transitions
// (0..1 linear-RGB) snap instead of ease. `EPS = 1e-4` is below 8-bit
// colour precision and sub-pixel position resolution, so a target
// delta under it is genuinely invisible. Duration rows carry no
// velocity, so this is a position-only check; curve completion is
// handled by the `t >= 1.0` arm in `AnimMapTyped::tick`, not here.
const SNAP_EPS_SQ: f32 = EPS * EPS;

/// Whether `secs` names a duration this crate will animate over.
pub(super) const fn duration_is_valid(secs: f32) -> bool {
    secs.is_finite() && secs >= 0.0 && secs <= MAX_DURATION_SECS
}

/// `displacement` is below the duration snap floor — the caller can
/// snap to target without animating, because the target barely moved.
/// Position-only (duration rows have no velocity). Consumed by the
/// duration arm of the snap-if-close fast path in `AnimMapTyped::tick`.
#[inline]
pub(super) fn within_duration_snap_eps<T: Animatable>(displacement: T) -> bool {
    displacement.magnitude_squared() < SNAP_EPS_SQ
}
