//! Duration motion model: the `(secs, ease)` validity bound and snap-if-close floor. Sibling of [`spring`](crate::animation::spring); each model owns its admission predicate and settle tolerance.

use crate::animation::animatable::Animatable;

use std::time::Duration;

/// The longest duration a spec runs for.
pub(super) const MAX_DURATION: Duration = Duration::from_secs(60);

const MAX_DURATION_SECS: f32 = MAX_DURATION.as_secs() as f32;

pub(super) const DURATION_ERROR: &str =
    "animation duration must be finite and in 0.0..=60.0 seconds";

/// Whether `secs` names a duration this crate will animate over.
pub(super) const fn duration_is_valid(secs: f32) -> bool {
    secs.is_finite() && secs >= 0.0 && secs <= MAX_DURATION_SECS
}

/// `displacement` is under the settle tolerance: the caller can snap to target, the target having barely moved. Used by the duration arm of the snap-if-close path in `AnimMapTyped::tick`.
///
/// The spring's tolerance ([`Animatable::settle_distance_squared`]): any visible change runs the full curve; only sub-perceptual drift (ulp rounding in theme math) snaps. Duration rows carry no velocity, so this is position-only, checked on retarget; completion is `MotionRow::advance`'s `progress >= 1` arm.
#[inline]
pub(super) fn within_duration_snap_eps<T: Animatable>(displacement: T) -> bool {
    displacement.settle_distance_squared() < 1.0
}
