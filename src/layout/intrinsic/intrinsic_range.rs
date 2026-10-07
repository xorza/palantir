//! The two intrinsic content sizes on one axis.

use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::len_req::LenReq;

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct IntrinsicRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl IntrinsicRange {
    pub(crate) const ZERO: Self = Self { min: 0.0, max: 0.0 };

    /// The half `req` names.
    #[inline]
    pub(crate) const fn get(self, req: LenReq) -> f32 {
        match req {
            LenReq::MinContent => self.min,
            LenReq::MaxContent => self.max,
        }
    }

    /// The `(kind, slot)` pairs `query` asks for, as mutable handles into this accumulator.
    ///
    /// Keeps the `query.includes(..)` gate every driver's `intrinsic` shares in one place, so a third `LenReq` isn't six silent call-site edits.
    #[inline]
    pub(crate) fn requested(
        &mut self,
        query: IntrinsicQuery,
    ) -> impl Iterator<Item = (LenReq, &mut f32)> {
        [
            (LenReq::MinContent, &mut self.min),
            (LenReq::MaxContent, &mut self.max),
        ]
        .into_iter()
        .filter(move |(req, _)| query.includes(*req))
    }
}
