//! The one antialiasing filter every edge is drawn through.

/// Half-width, in physical pixels, of the box filter every edge is antialiased with; the shaders take it through the shared prelude.
pub(crate) const AA_HALF_WIDTH: f32 = 0.5;

// The quad shader and composer culling assume the ramp reaches no further than half a pixel.
const _: () = assert!(AA_HALF_WIDTH <= 0.5);
