//! What a shaped run measured to: its block, and the ink past it.

use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;

/// The block a shaped run lays out to (glyph advances) and how far its ink reaches past it on each side, as in an italic overhang; damage and the text scissor cover the ink. Whole pixels, exact in f16 to 2048 px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextExtent {
    pub(crate) size: Size,
    pub(crate) ink: Spacing,
}

impl TextExtent {
    /// A block whose ink stays inside it.
    pub(crate) const fn inked_within(size: Size) -> Self {
        Self {
            size,
            ink: Spacing::ZERO,
        }
    }
}
