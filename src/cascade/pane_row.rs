//! One backdrop pane as damage and the encoder see it.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;

/// A pane of one plane: who it is, where it stands in paint order, and the screen it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaneRow {
    pub(crate) key: PaneKey,
    /// The pane's row in the plane's `paint_arena.rows`. Everything painted before the pane has a
    /// lower row, so a lower row reads as before it; a later shape of an ancestor does too, which
    /// costs a recapture at worst.
    pub(crate) row: u32,
    /// The pane's screen rect, its paint row's.
    pub(crate) rect: Rect,
    /// The screen a capture of the pane reads: `rect` grown by its blur's reach and a pixel, for
    /// the edge the composer rounds out.
    pub(crate) footprint: Rect,
}

impl PaneRow {
    /// The footprint of a pane at `rect` with `backdrop`, its shapes scaled by `scale` onto a
    /// surface of `display_scale` physical px per logical px.
    pub(crate) fn footprint_of(
        rect: Rect,
        backdrop: LoweredBackdrop,
        scale: f32,
        display_scale: f32,
    ) -> Rect {
        let physical = scale * display_scale;
        let blur = BlurPlan::new(backdrop.sigma() * physical).reach() as f32;
        let reach = blur + backdrop.lens_reach(physical) as f32 + 1.0;
        rect.inflated(reach / display_scale)
    }
}
