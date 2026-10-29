//! The one antialiasing filter every edge is drawn through.

use crate::primitives::geometry::rect::Rect;

/// Half-width, in physical pixels, of the box filter every edge is antialiased with; the shaders take it through the shared prelude.
pub(crate) const AA_HALF_WIDTH: f32 = 0.5;

// The quad shader and composer culling assume the ramp reaches no further than half a pixel.
const _: () = assert!(AA_HALF_WIDTH <= 0.5);

/// The bound a rect turned `angle` radians about its center shades: its ramp reaches
/// [`AA_HALF_WIDTH`] past each edge in its own frame, as the quad and image shaders draw it.
pub(crate) fn turned_reach(rect: Rect, angle: f32) -> Rect {
    rect.inflated(AA_HALF_WIDTH)
        .turned_cover(rect.center(), angle)
}
