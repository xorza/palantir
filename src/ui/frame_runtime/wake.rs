//! The `FrameRuntime` repaint-wake queue's entry type, and the cause bitset
//! each entry carries.

use std::time::Duration;

/// Bitset over wake causes, OR-merged when requests coalesce onto one deadline slot so
/// `FrameRuntime::take_frame_plan` sees every reason when it chooses
/// [`FramePlan::PaintOnly`](crate::ui::frame_runtime::FramePlan::PaintOnly) or
/// [`FramePlan::FullRecord`](crate::ui::frame_runtime::FramePlan::FullRecord).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct WakeReasons(u8);

impl WakeReasons {
    /// A wake via `Ui::request_repaint_after` (spring tick, host schedule, widget owing a paint);
    /// needs a full record.
    pub(crate) const REAL: Self = Self(1 << 0);
    /// Paint-animation quantum boundary, filed in `WindowFrames::frame` from
    /// `Cascade::min_paint_animation_wake`; alone it needs only damage and paint, reusing the prior
    /// record.
    pub(crate) const ANIMATION: Self = Self(1 << 1);

    #[inline]
    pub(super) const fn merge(self, r: Self) -> Self {
        Self(self.0 | r.0)
    }

    /// `true` when `ANIMATION` is the only reason; gates `FramePlan::PaintOnly`.
    #[inline]
    pub(super) fn is_animation_only(self) -> bool {
        self == Self::ANIMATION
    }
}

/// One entry on the `FrameRuntime` repaint-wake queue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}
