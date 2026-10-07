//! The `FrameRuntime` repaint-wake queue's entry type, and the cause bitset
//! each entry carries.

use std::time::Duration;

/// Bitset over wake causes, OR-merged when requests coalesce onto one deadline slot so the frame-entry classifier sees every reason (choosing [`FramePlan::PaintOnly`](crate::ui::frame_runtime::FramePlan::PaintOnly) or [`FramePlan::FullRecord`](crate::ui::frame_runtime::FramePlan::FullRecord) in `FrameRuntime::take_frame_plan`). A bitset because one deadline can carry both.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct WakeReasons(u8);

impl WakeReasons {
    /// A wake via `Ui::request_repaint_after` (spring tick, host schedule, widget owing a paint); needs a full record + measure + arrange + cascade pass.
    pub(crate) const REAL: Self = Self(1 << 0);
    /// Paint-anim quantum boundary, filed in `FrameCycle::run` from `Forest::min_paint_anim_wake`; alone it needs only damage compute + paint, reusing the prior frame's record output.
    pub(crate) const ANIM: Self = Self(1 << 1);

    #[inline]
    pub(super) const fn merge(self, r: Self) -> Self {
        Self(self.0 | r.0)
    }

    /// `true` when `ANIM` is the only reason; gates `FrameProcessing::PaintOnly`.
    #[inline]
    pub(super) fn is_anim_only(self) -> bool {
        self == Self::ANIM
    }
}

/// One entry on the `FrameRuntime` repaint-wake queue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}
