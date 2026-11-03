//! What a shaped run measured to: its block, and the ink past it.

use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::text::key::TextShapeKey;

/// The block a shaped run lays out to (glyph advances) and how far its ink reaches past it on each side, as in an italic overhang; damage and the text scissor cover the ink. Whole pixels, exact in f16 to 2048 px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextExtent {
    pub(crate) size: Size,
    pub(crate) ink: Spacing,
}

/// The height `line_count` lines of `line_height` take in a block: whole pixels, rounded up, as a
/// [`TextExtent`] holds a size. The one rule a shaped block, the mono metric and a field's one-line
/// floor measure by, so an empty field and its first character take one height. `line_height` is
/// on the shaping key's grid, as [`TextShapeKey::leading_on_grid`] snaps it.
/// [`TextStyle::block_height`](crate::TextStyle::block_height) is its public form.
pub(crate) fn block_height(line_height: f32, line_count: usize) -> f32 {
    debug_assert_eq!(
        TextShapeKey::leading_on_grid(line_height),
        line_height,
        "a line height off the shaping key's grid"
    );
    (line_count as f32 * line_height).ceil()
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
