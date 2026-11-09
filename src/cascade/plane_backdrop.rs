//! One backdrop as damage, the backdrop ledger and the encoder see it.

use crate::display::Display;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::shape::paint::blur_plan::BlurPlan;

/// A backdrop of one plane: who it is, where it stands in paint order, its blur, and the screen it
/// reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaneBackdrop {
    pub(crate) key: BackdropKey,
    /// The backdrop's row in the plane's `paint_arena.rows`.
    pub(crate) row: u32,
    /// Where the backdrop stands in paint order, as a row of `paint_arena.rows`: every row below it
    /// paints before the backdrop. Its own `row` for a chrome or a shape before its node's
    /// children; for a shape after a child, the first row past that child's subtree, as the rows
    /// lie in preorder, a node's before its children's, so the child's rows sit above the
    /// backdrop's own. A row its node paints after the backdrop, lower than its children's, still
    /// reads as before it, which costs a recapture at worst.
    pub(crate) paint_row: u32,
    /// The backdrop's screen rect, its paint row's.
    pub(crate) rect: Rect,
    /// The blur of the backdrop's σ in physical px, taken in f32.
    pub(crate) plan: BlurPlan,
    /// The screen a capture of the backdrop reads: `rect` grown by [`Self::reach`]. The one
    /// footprint of the backdrop: damage reads it, the encoder keeps the draws under it, and the
    /// capture copies it.
    pub(crate) footprint: Rect,
}

impl PlaneBackdrop {
    /// How far past a backdrop's rect its capture reads, in physical px: its blur's reach, its
    /// lens's, and a pixel for the edge the composer rounds out.
    pub(crate) fn reach(plan: BlurPlan, lens_margin: u32) -> f32 {
        plan.reach() as f32 + lens_margin as f32 + 1.0
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
