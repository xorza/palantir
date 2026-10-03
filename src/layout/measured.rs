//! What one measure produced: an extent, the least of it the content
//! can give way to, and the offers it holds under.

use crate::primitives::size::Size;

/// An extent, its floor, and where it holds, from one measure.
///
/// A driver answers with its content and the content's floor, before
/// padding and margin, and with `stable_from` in the space of the
/// `inner_avail` it was handed;
/// [`LayoutPass::measure`](crate::layout::pass::LayoutPass::measure)
/// answers with the node's margin-inclusive extent and floor, and with
/// `stable_from` in the space of the `available` it was offered.
///
/// The floor is the smallest extent the subtree can take at the
/// constraints it was measured under without its content overflowing:
/// wrapped text at the width it was shaped to, fixed extents, the floors
/// of its children. Content that gives way — a scroll on a panned axis —
/// contributes nothing. It is what a `Hug` axis shrinks no further than.
/// Never larger than `size`.
///
/// `stable_from` is, per axis, the least finite offer from which the
/// measure holds unchanged — every finite offer at or past it measures
/// the whole subtree to the same result, which is what lets the measure
/// cache serve the subtree under an offer it was not measured at. Zero
/// when no finite offer changes it, [`Self::AT_OFFER_ONLY`] when it holds
/// at the offer it was taken at alone.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Measured {
    pub(super) size: Size,
    pub(super) floor: Size,
    pub(super) stable_from: Size,
}

impl Measured {
    /// The `stable_from` of an axis whose measure read its offer — a Fill
    /// share, a line break, text bound to a width. No finite offer reaches
    /// it, so the cache serves the axis at its own offer alone.
    pub(crate) const AT_OFFER_ONLY: f32 = f32::INFINITY;

    pub(super) const ZERO: Self = Self {
        size: Size::ZERO,
        floor: Size::ZERO,
        stable_from: Size::ZERO,
    };
}
