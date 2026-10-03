//! What a shaped run measured to: its block, and the ink past it.

use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;

/// The block a shaped run lays out to, and how far its glyphs' ink
/// reaches past that block on each side.
///
/// Layout sizes and places the block, which spans the glyphs' advances.
/// The ink is what paints: an italic's overhang or a negative side
/// bearing reaches past the block, and what covers the painted pixels —
/// damage, the text scissor — has to cover that too. Whole pixels, so the
/// f16 lanes hold it exactly to 2048 px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextExtent {
    pub(crate) size: Size,
    pub(crate) ink: Spacing,
}

impl TextExtent {
    /// A block whose ink stays inside it — what a measure that reads no
    /// glyph outlines answers.
    pub(crate) const fn inked_within(size: Size) -> Self {
        Self {
            size,
            ink: Spacing::ZERO,
        }
    }
}
