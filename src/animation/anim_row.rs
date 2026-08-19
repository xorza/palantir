//! One animated value's cross-frame row: where it is, where it is going,
//! and the motion model carrying it there.

use crate::animation::anim_spec::AnimMotion;
use crate::animation::animatable::Animatable;

/// State carried only by the active motion model. The variant is also
/// the row's mode tag, so duration and spring state cannot drift apart.
#[derive(Clone, Copy, Debug)]
pub(super) enum MotionRow<T: Animatable> {
    Duration { segment_start: T, elapsed: f32 },
    Spring { velocity: T },
}

impl<T: Animatable> MotionRow<T> {
    pub(super) fn new(motion: AnimMotion, current: &T) -> Self {
        match motion {
            AnimMotion::Duration { .. } => Self::Duration {
                segment_start: current.clone(),
                elapsed: 0.0,
            },
            AnimMotion::Spring { .. } => Self::Spring {
                velocity: T::zero(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimRow<T: Animatable> {
    pub(super) current: T,
    pub(super) target: T,
    pub(super) motion: MotionRow<T>,
    /// Set by every `tick`, cleared by `post_record`. Rows still
    /// `false` at `post_record` are dropped — that's how a slot whose
    /// caller stopped poking it (widget id stuck around but the
    /// animation site went away) gets evicted. Without this the
    /// `(WidgetId, AnimSlot)` map only shrinks on full widget removal.
    pub(super) touched: bool,
    /// `Ui` render-frame id at the last `tick` that ran the integrator
    /// step. A second `tick` in the same frame (multi-pass record:
    /// the frame driver re-runs `build` after an input action drains) sees
    /// this match and short-circuits the dt-driven advance, so the
    /// integrator advances exactly once per host frame. Retarget
    /// logic still runs in the short-circuited call so pass B's
    /// post-action target replaces pass A's stale one.
    pub(super) advanced_at: u64,
    /// Cached settle state, set true on insert / when the integrator
    /// or `within_settle_eps` confirms settlement, false on retarget.
    /// Lets `tick` fast-return on a steady-state row without the
    /// `sub` + `magnitude_squared` settle math; the `PartialEq`
    /// retarget compare still runs so a target change unfreezes the
    /// row immediately.
    pub(super) settled: bool,
}
