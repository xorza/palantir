//! Which intrinsic content size a query asks for.

use crate::primitives::layout::axis::Axis;

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum LenReq {
    /// Smallest size without breaking; text: longest unbreakable run.
    MinContent,
    /// Size wanted with unlimited room; text: natural unbroken width.
    MaxContent,
}

/// Width of `LayoutScratch.intrinsics`' `[f32; SLOT_COUNT]`: `LenReq` × `Axis` variants; the `const _:` below catches overflow.
pub(crate) const SLOT_COUNT: usize = 4;

impl LenReq {
    #[inline]
    pub(crate) const fn slot(self, axis: Axis) -> usize {
        let a = match axis {
            Axis::X => 0,
            Axis::Y => 1,
        };
        let r = match self {
            LenReq::MinContent => 0,
            LenReq::MaxContent => 1,
        };
        a * 2 + r
    }
}

const _: () = {
    assert!(LenReq::MinContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MinContent.slot(Axis::Y) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::Y) < SLOT_COUNT);
};
