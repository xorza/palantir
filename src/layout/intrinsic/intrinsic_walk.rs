//! What one intrinsic walk answered, per axis.

use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;

/// A driver answers only the axis it was asked; a leaf answers both, as its content is a `Size` of min and max-content.
///
/// [`LayoutPass::measure`](crate::layout::pass::LayoutPass) asks min-content on both axes; carrying the free lane back lets the second query read the slot array instead of reshaping.
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
