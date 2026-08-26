//! Why a repaint wake was filed.

/// Bitset over wake causes. OR-merged when two requests coalesce
/// onto the same deadline slot, so the frame-entry classifier can see
/// every reason behind a fired wake — which is what picks
/// [`FramePlan::PaintOnly`](crate::ui::frame_plan::FramePlan::PaintOnly)
/// over [`FramePlan::FullRecord`](crate::ui::frame_plan::FramePlan::FullRecord)
/// in `FrameRuntime::take_frame_plan`. Bit set, not enum, because
/// a single deadline can legitimately have both bits at once
/// (paint-anim quantum aligning with a widget-scheduled wake).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct WakeReasons(u8);

impl WakeReasons {
    /// Caller asked for a wake via `Ui::request_repaint_after` —
    /// state-spring tick, host-driven schedule, widget that owes a
    /// future paint. Requires a full record + measure + arrange +
    /// cascade pass.
    pub(crate) const REAL: Self = Self(1 << 0);
    /// Paint-anim quantum boundary, filed in `FrameCycle::run` from
    /// `Forest::min_paint_anim_wake`. On its own, only needs a
    /// damage compute + paint — record/post-record output from the
    /// prior frame is reused as-is.
    pub(crate) const ANIM: Self = Self(1 << 1);

    #[inline]
    pub(super) fn merge(self, r: Self) -> Self {
        Self(self.0 | r.0)
    }

    /// `true` when the only reason set is `ANIM` — the predicate that
    /// gates `FrameProcessing::PaintOnly`.
    #[inline]
    pub(super) fn is_anim_only(self) -> bool {
        self == Self::ANIM
    }
}
