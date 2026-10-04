//! The one antialiasing filter every edge is drawn through.

/// Half-width, in physical pixels, of the box filter every edge is
/// antialiased with: a pixel's coverage is the share of the
/// `2 · AA_HALF_WIDTH` span about its centre, across the edge, that the
/// shape covers. So a pixel is fully covered from this far inside an
/// edge, uncovered from this far outside it, and nothing an edge paints
/// reaches further out. The shaders take it through the shared prelude.
pub(crate) const AA_HALF_WIDTH: f32 = 0.5;

// The quad shader grows each quad to the pixel centres within
// `AA_HALF_WIDTH` of its rect, and the composer culls and tracks the quad
// by the whole pixels that cover the rect alone. Those hold every such
// centre only while the ramp reaches no further than half a pixel.
const _: () = assert!(AA_HALF_WIDTH <= 0.5);
