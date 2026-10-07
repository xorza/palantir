//! What one measure produced: an extent, its floor, and the offers it holds under.

use crate::primitives::geometry::size::Size;

/// An extent, its floor, and where it holds, from one measure.
///
/// A driver answers before padding and margin, with `stable_from` in the space of its `inner_avail`;
/// [`LayoutPass::measure`](crate::layout::pass::LayoutPass::measure) answers margin-inclusive, in the space of the `available` offered.
///
/// The floor is the smallest extent without overflow (wrapped text at its shaped width, fixed extents, child floors); giving-way content such as a scroll's panned axis adds nothing. A `Hug` axis shrinks no further. Never above `size`.
///
/// `stable_from` is per axis the least finite offer from which every larger finite offer measures the same, so the measure cache can serve other offers. Zero when no finite offer changes it, [`Self::AT_OFFER_ONLY`] when only the measured offer holds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Measured {
    pub(super) size: Size,
    pub(super) floor: Size,
    pub(super) stable_from: Size,
}

impl Measured {
    /// `stable_from` of an axis that read its offer (Fill share, line break, width-bound text); no finite offer reaches it, so the cache serves only the same offer.
    pub(crate) const AT_OFFER_ONLY: f32 = f32::INFINITY;

    pub(super) const ZERO: Self = Self {
        size: Size::ZERO,
        floor: Size::ZERO,
        stable_from: Size::ZERO,
    };
}
