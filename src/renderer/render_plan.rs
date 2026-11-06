//! Internal renderer work selected after scene damage classification.

use crate::damage::Damage;
use crate::primitives::paint::color::RgbaF32;

/// WindowDriver-facing render plan, present only when there is render work; `FrameReport.plan = None` is the skip signal. Pairs the clear color with the frame's [`Damage`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPlan {
    /// Surface clear color for this frame.
    pub(crate) clear: RgbaF32,
    /// Whole surface or a damage region. `Partial` loads the backbuffer and paints inside the rects after a clear-colored pre-fill per scissor; coverage rides along for the present-path promote decision (`host::window_driver::promote_share`).
    pub(crate) damage: Damage,
}

impl RenderPlan {
    /// This plan escalated to a full repaint, keeping its clear color; for when partial damage can't be honored (direct present, fresh backbuffer).
    pub(crate) const fn to_full(self) -> Self {
        Self {
            clear: self.clear,
            damage: Damage::Full,
        }
    }
}
