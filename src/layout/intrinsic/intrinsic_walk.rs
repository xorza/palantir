//! What one intrinsic walk answered, per axis.

use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;

/// What one intrinsic walk answered, per axis.
///
/// A driver answers the axis it was asked about and nothing else — "how
/// tall given you pack across" is a different recursion from "how wide",
/// so the other axis costs a second walk. A leaf answers both at once:
/// its content is a shaped run's min-content and max-content, and those
/// are `Size`s, so the axis a query names only picks a lane of a value
/// the walk already holds.
///
/// [`LayoutPass::measure`](crate::layout::pass::LayoutPass) asks every
/// node for min-content on both axes. Carrying the free lane back is
/// what lets the engine record it, so the second of those two queries
/// reads the frame's slot array instead of shaping the leaf's runs
/// again.
#[derive(Copy, Clone, Debug)]
pub(crate) struct IntrinsicWalk {
    /// The axis the query named.
    pub(crate) answered: IntrinsicRange,
    /// The other axis, when the walk covered it for free.
    pub(crate) sibling: Option<IntrinsicRange>,
}

impl IntrinsicWalk {
    #[inline]
    pub(super) const fn one_axis(answered: IntrinsicRange) -> Self {
        Self {
            answered,
            sibling: None,
        }
    }
}
