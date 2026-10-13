//! Which content sizes one intrinsic query asks for, and on which axis.

use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::layout::axis::Axis;

/// Which content sizes one query asks for on one axis: a single [`LenReq`], or both in one recursion (`None`).
///
/// **A runtime field on purpose.** A `const RANGE: bool` was tried; the mode is constant per query tree so the branch predicts perfectly, while the second monomorphization doubles the recursion's code. Removing it made `grid/intrinsic/forced_miss` ~9% faster. Re-specializing needs a measurement that says otherwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery {
    axis: Axis,
    single_req: Option<LenReq>,
}

impl IntrinsicQuery {
    pub(crate) const fn single(axis: Axis, req: LenReq) -> Self {
        Self {
            axis,
            single_req: Some(req),
        }
    }

    pub(crate) const fn range(axis: Axis) -> Self {
        Self {
            axis,
            single_req: None,
        }
    }

    /// The axis every node of the walk is asked about.
    #[inline]
    pub(crate) const fn axis(self) -> Axis {
        self.axis
    }

    #[inline]
    pub(crate) const fn includes(self, req: LenReq) -> bool {
        match self.single_req {
            Some(single) => single as u8 == req as u8,
            None => true,
        }
    }

    /// The same halves on the other axis.
    #[inline]
    pub(crate) const fn across(self) -> Self {
        Self {
            axis: self.axis.other(),
            single_req: self.single_req,
        }
    }

    /// The halves of this query `keep` accepts, or `None` when it accepts neither; lets a caller that discards one half narrow the recursion.
    pub(crate) fn retain(self, mut keep: impl FnMut(LenReq) -> bool) -> Option<Self> {
        let min = self.includes(LenReq::MinContent) && keep(LenReq::MinContent);
        let max = self.includes(LenReq::MaxContent) && keep(LenReq::MaxContent);
        let single_req = match (min, max) {
            (true, true) => None,
            (true, false) => Some(LenReq::MinContent),
            (false, true) => Some(LenReq::MaxContent),
            (false, false) => return None,
        };
        Some(Self {
            axis: self.axis,
            single_req,
        })
    }
}
