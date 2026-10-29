//! One backdrop pane as damage, the pane ledger and the encoder see it.

use crate::display::Display;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::shape::paint::blur_plan::BlurPlan;

/// A pane of one plane: who it is, where it stands in paint order, its blur, and the screen it
/// reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaneRow {
    pub(crate) key: PaneKey,
    /// The pane's row in the plane's `paint_arena.rows`. Everything painted before the pane has a
    /// lower row, so a lower row reads as before it; a later shape of an ancestor does too, which
    /// costs a recapture at worst.
    pub(crate) row: u32,
    /// The pane's screen rect, its paint row's.
    pub(crate) rect: Rect,
    /// The blur of the pane's σ in physical px, taken in f32.
    pub(crate) plan: BlurPlan,
    /// The screen a capture of the pane reads: `rect` grown by [`Self::reach`]. The one footprint
    /// of the pane: damage reads it, the encoder keeps the draws under it, and the capture copies
    /// it.
    pub(crate) footprint: Rect,
}

impl PaneRow {
    /// How far past a pane's rect its capture reads, in physical px: its blur's reach, its lens's,
    /// and a pixel for the edge the composer rounds out.
    pub(crate) fn reach(plan: BlurPlan, lens_reach: u32) -> f32 {
        plan.reach() as f32 + lens_reach as f32 + 1.0
    }

    /// The physical px of [`Self::footprint`] on `display`: its edges rounded out, inside the
    /// surface.
    pub(crate) fn physical_footprint(&self, display: Display) -> URect {
        let scale = display.scale_factor();
        let surface = display.physical;
        URect::covering(Rect::from_min_max(
            self.footprint.min * scale,
            self.footprint.max() * scale,
        ))
        .clamp_to(URect::new(0, 0, surface.x, surface.y))
    }
}
