//! Internal renderer work selected after scene damage classification.

use crate::primitives::color::Color;
use crate::scene::damage::Damage;
use crate::scene::damage::region::DamageRegion;

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
    /// Load the backbuffer, then paint inside `region` after a
    /// `clear`-coloured pre-fill quad per scissor.
    Partial { region: DamageRegion },
}

impl RenderPlan {
    /// Build a render plan from `DamageEngine`'s output plus the
    /// surface clear colour. `Damage::Skip` ⇒ `None` (skip frame);
    /// `Full` / `Partial` ⇒ `Some(plan)`.
    pub(crate) fn from_damage(damage: Damage, clear: Color) -> Option<Self> {
        let kind = match damage {
            Damage::Skip => return None,
            Damage::Full => RenderKind::Full,
            Damage::Partial(region) => RenderKind::Partial { region },
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
