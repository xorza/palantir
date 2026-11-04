//! Which content sizes one intrinsic query asks for, and on which axis.

use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::layout::axis::Axis;

/// Which content sizes one query asks for on one axis: one [`LenReq`], or both in one recursion.
///
/// **A runtime field on purpose.** A `const RANGE: bool` was tried; the mode is constant per query tree so the branch predicts perfectly, while the second monomorphization doubles the recursion's code. Removing it made `grid/intrinsic/forced_miss` ~9% faster. Re-specializing needs a measurement that says otherwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery {
    axis: Axis,
    reqs: LenReqs,
}

/// The content sizes a query asks for, never none: a query that would ask for none is not made
/// ([`IntrinsicQuery::retain`]). Refused: an `Option<LenReq>` whose `None` meant both, and a flag
/// set, which can hold none.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum LenReqs {
    Min,
    Max,
    Both,
}

impl LenReqs {
    const fn one(req: LenReq) -> Self {
        match req {
            LenReq::MinContent => Self::Min,
            LenReq::MaxContent => Self::Max,
        }
    }

    const fn includes(self, req: LenReq) -> bool {
        match (self, req) {
            (Self::Both, _) | (Self::Min, LenReq::MinContent) | (Self::Max, LenReq::MaxContent) => {
                true
            }
            (Self::Min, LenReq::MaxContent) | (Self::Max, LenReq::MinContent) => false,
        }
    }
}

impl IntrinsicQuery {
    pub(crate) const fn single(axis: Axis, req: LenReq) -> Self {
        Self {
            axis,
            reqs: LenReqs::one(req),
        }
    }

    pub(crate) const fn range(axis: Axis) -> Self {
        Self {
            axis,
            reqs: LenReqs::Both,
        }
    }

    /// The axis every node of the walk is asked about.
    #[inline]
    pub(crate) const fn axis(self) -> Axis {
        self.axis
    }

    #[inline]
    pub(crate) const fn includes(self, req: LenReq) -> bool {
        self.reqs.includes(req)
    }

    /// The same halves on the other axis.
    #[inline]
    pub(crate) const fn across(self) -> Self {
        Self {
            axis: self.axis.other(),
            reqs: self.reqs,
        }
    }

    /// The halves of this query `keep` accepts, or `None` when it accepts neither; lets a caller that discards one half narrow the recursion.
    pub(crate) fn retain(self, mut keep: impl FnMut(LenReq) -> bool) -> Option<Self> {
        let min = self.includes(LenReq::MinContent) && keep(LenReq::MinContent);
        let max = self.includes(LenReq::MaxContent) && keep(LenReq::MaxContent);
        let reqs = match (min, max) {
            (true, true) => LenReqs::Both,
            (true, false) => LenReqs::Min,
            (false, true) => LenReqs::Max,
            (false, false) => return None,
        };
        Some(Self {
            axis: self.axis,
            reqs,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
    use crate::layout::intrinsic::len_req::LenReq::{MaxContent, MinContent};
    use crate::primitives::layout::axis::Axis;

    /// A single query includes its own size alone and a range both; `retain` keeps the halves
    /// both the query and `keep` take, and none of them makes no query; `across` keeps the halves.
    #[test]
    fn a_query_keeps_exactly_the_halves_asked_for() {
        let min = IntrinsicQuery::single(Axis::X, MinContent);
        let range = IntrinsicQuery::range(Axis::X);
        assert!(min.includes(MinContent));
        assert!(!min.includes(MaxContent));
        assert!(range.includes(MinContent));
        assert!(range.includes(MaxContent));

        assert_eq!(
            range.retain(|req| req == MaxContent),
            Some(IntrinsicQuery::single(Axis::X, MaxContent))
        );
        assert_eq!(range.retain(|_| true), Some(range));
        assert_eq!(range.retain(|_| false), None);
        assert_eq!(min.retain(|_| true), Some(min));
        assert_eq!(min.retain(|req| req == MaxContent), None);

        assert_eq!(min.across(), IntrinsicQuery::single(Axis::Y, MinContent));
        assert_eq!(range.across(), IntrinsicQuery::range(Axis::Y));
    }
}
