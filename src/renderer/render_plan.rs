//! Internal renderer work selected after scene damage classification.

use crate::primitives::color::Color;
use crate::scene::damage::Damage;
use crate::scene::damage::region::CollapsedDamage;

/// WindowDriver-facing render plan, present only when there's actual render
/// work this frame — `FrameReport.plan = None` is the skip signal, so neither
/// the encoder nor the backend ever sees a no-op plan. Pairs the surface clear
/// colour (needed for both kinds: `Full` clears the colour attachment,
/// `Partial` pre-fills each scissor with it) with the [`RenderKind`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPlan {
    /// Surface clear colour for this frame.
    pub(crate) clear: Color,
    /// Whole surface, or just a damage region.
    pub(crate) kind: RenderKind,
}

/// What a [`RenderPlan`] repaints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RenderKind {
    /// Clear + repaint the whole surface.
    Full,
    /// Load the backbuffer, then paint inside the damage rects after a
    /// `clear`-coloured pre-fill quad per scissor. The coverage rides
    /// along for the present-path promote decision — see
    /// `DIRECT_PROMOTE_COVERAGE`.
    Partial { damage: CollapsedDamage },
}

impl RenderKind {
    /// Whether the frame paints inside damage rects rather than over the
    /// whole surface.
    ///
    /// The plan is the authority. `build_repaint_scissors` maps this
    /// one-to-one onto a `RepaintScissors`, so asking the *scissors* gives
    /// the same answer one derivation further from the fact.
    pub(crate) fn is_partial(self) -> bool {
        matches!(self, Self::Partial { .. })
    }
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

    /// Build a render plan from `DamageEngine`'s output plus the
    /// surface clear colour. `Damage::Skip` ⇒ `None` (skip frame);
    /// `Full` / `Partial` ⇒ `Some(plan)`.
    pub(crate) fn from_damage(damage: Damage, clear: Color) -> Option<Self> {
        let kind = match damage {
            Damage::Skip => return None,
            Damage::Full => RenderKind::Full,
            Damage::Partial(damage) => RenderKind::Partial { damage },
        };
        Some(RenderPlan { clear, kind })
    }

    /// This plan escalated to a full repaint, keeping its clear colour — used
    /// when partial damage can't be honoured (direct present, or a freshly
    /// (re)created backbuffer with undefined contents).
    pub(crate) fn to_full(self) -> RenderPlan {
        RenderPlan {
            clear: self.clear,
            kind: RenderKind::Full,
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
