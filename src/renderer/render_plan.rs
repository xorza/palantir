//! Internal renderer work selected after scene damage classification.

use crate::primitives::color::RgbaF32;
use crate::scene::damage::Damage;

/// WindowDriver-facing render plan, present only when there's actual render
/// work this frame — `FrameReport.plan = None` is the skip signal, so neither
/// the encoder nor the backend ever sees a no-op plan. Pairs the surface clear
/// colour (needed for both outcomes: `Full` clears the colour attachment,
/// `Partial` pre-fills each scissor with it) with the frame's [`Damage`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPlan {
    /// Surface clear colour for this frame.
    pub(crate) clear: RgbaF32,
    /// Whole surface, or just a damage region. `Full` clears and repaints
    /// everything; `Partial` loads the backbuffer and paints inside the
    /// rects after a `clear`-coloured pre-fill quad per scissor, with the
    /// coverage riding along for the present-path promote decision (see
    /// `DIRECT_PROMOTE_COVERAGE`).
    ///
    /// The scene's own outcome, carried rather than restated: `Damage`'s
    /// "nothing to paint" is already the absence of one, which is what
    /// this plan's `Option` says too.
    pub(crate) damage: Damage,
}

impl RenderPlan {
    /// Physical-pixel padding around every partial-repaint scissor for
    /// antialiasing fringes and glyph overhang. The backend inflates
    /// each scissor by this much; [`Self::cull_margin`] is the logical
    /// slack the frontend must match so it never culls a draw that
    /// lands inside the padded rect.
    pub(crate) const AA_PADDING: u32 = 2;

    /// Logical-pixel culling slack matching the backend's scissor
    /// padded by [`Self::AA_PADDING`].
    pub(crate) fn cull_margin(scale: f32) -> f32 {
        (Self::AA_PADDING as f32 + 1.0) / scale
    }

    /// Stamp `DamageEngine`'s output with the surface clear colour. A
    /// frame with no damage stays `None` all the way to the host.
    pub(crate) fn from_damage(damage: Option<Damage>, clear: RgbaF32) -> Option<Self> {
        Some(RenderPlan {
            clear,
            damage: damage?,
        })
    }

    /// This plan escalated to a full repaint, keeping its clear colour — used
    /// when partial damage can't be honoured (direct present, or a freshly
    /// (re)created backbuffer with undefined contents).
    pub(crate) fn to_full(self) -> RenderPlan {
        RenderPlan {
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
