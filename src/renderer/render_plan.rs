//! Internal renderer work selected after scene damage classification.

use crate::damage::Damage;
use crate::primitives::paint::color::RgbaF32;

/// WindowDriver-facing render plan, present only when there is render work; `FrameReport.plan = None` is the skip signal. Pairs the clear color with the frame's [`Damage`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPlan {
    /// Surface clear color for this frame.
    pub(crate) clear: RgbaF32,
    /// Whole surface or a damage region. `Partial` loads the backbuffer and paints inside the rects after a clear-colored pre-fill per scissor; coverage rides along for the present-path promote decision (`DIRECT_PROMOTE_COVERAGE`).
    pub(crate) damage: Damage,
}

impl RenderPlan {
    /// Physical-pixel padding around each partial scissor for AA fringes and glyph overhang; [`Self::cull_margin`] is the logical slack the frontend must match.
    pub(crate) const AA_PADDING: u32 = 2;

    /// Logical-pixel culling slack matching the scissor padding [`Self::AA_PADDING`].
    pub(crate) const fn cull_margin(scale: f32) -> f32 {
        (Self::AA_PADDING as f32 + 1.0) / scale
    }

    /// Stamp `DamageEngine`'s output with the clear color; no damage stays `None`.
    pub(crate) fn from_damage(damage: Option<Damage>, clear: RgbaF32) -> Option<Self> {
        Some(Self {
            clear,
            damage: damage?,
        })
    }

    /// This plan escalated to a full repaint, keeping its clear color; for when partial damage can't be honored (direct present, fresh backbuffer).
    pub(crate) const fn to_full(self) -> Self {
        Self {
            clear: self.clear,
            damage: Damage::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::render_plan::RenderPlan;

    #[test]
    fn cull_margin_scales_inversely() {
        assert_eq!(RenderPlan::cull_margin(1.0), 3.0);
        assert_eq!(RenderPlan::cull_margin(2.0), 1.5);
    }
}
