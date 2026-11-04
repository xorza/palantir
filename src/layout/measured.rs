//! What one measure produced: an extent, its floor, and the offers it holds under, in the
//! space of a node ([`Measured`]) and of a driver's content ([`ContentMeasure`]).

use crate::primitives::geometry::size::Size;

/// A node's extent, its floor, and where it holds, from one measure: margin-inclusive, in the
/// space of the `available` offered, as [`LayoutPass::measure`](crate::layout::pass::LayoutPass::measure)
/// answers and the measure cache keeps it.
///
/// The floor is the smallest extent without overflow (wrapped text at its shaped width, fixed extents, child floors); giving-way content such as a scroll's panned axis or a canvas's bounded one, where content overflows, adds nothing. A `Hug` axis shrinks no further. Never above `size`.
///
/// `stable_from` is per axis the least finite offer from which every larger finite offer measures the same, so the measure cache can serve other offers. Zero when no finite offer changes it, [`offer::AT_OFFER_ONLY`](crate::primitives::layout::offer::AT_OFFER_ONLY) when only the measured offer holds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Measured {
    pub(super) size: Size,
    pub(super) floor: Size,
    pub(super) stable_from: Size,
}

impl Measured {
    pub(super) const ZERO: Self = Self {
        size: Size::ZERO,
        floor: Size::ZERO,
        stable_from: Size::ZERO,
    };
}

/// A driver's answer, as [`Measured`] reads but before padding, margin and clamping, with
/// `stable_from` in the space of its `inner_avail`; `NodeSlot::resolve` folds it into the node's
/// [`Measured`]. Its own type, so a node's answer cannot pass for its content's: the two spaces
/// differ by padding and margin. Refused: one type whose space only prose told apart.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentMeasure {
    pub(super) size: Size,
    pub(super) floor: Size,
    pub(super) stable_from: Size,
}

impl ContentMeasure {
    pub(super) const ZERO: Self = Self {
        size: Size::ZERO,
        floor: Size::ZERO,
        stable_from: Size::ZERO,
    };
}
