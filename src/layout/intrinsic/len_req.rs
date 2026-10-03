//! Which intrinsic content size a query asks for.

use crate::primitives::layout::axis::Axis;

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum LenReq {
    /// Smallest size the node can occupy without breaking. Text: longest
    /// unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural unbroken
    /// width.
    MaxContent,
}

/// Width of the `[f32; SLOT_COUNT]` array on `LayoutScratch.intrinsics`.
/// Equals `LenReq` variants × `Axis` variants. Adding a third variant
/// to either enum must update this constant and `LenReq::slot`; the
/// `const _:` below catches the array overflow at compile time.
pub(crate) const SLOT_COUNT: usize = 4;

impl LenReq {
    /// Index into `LayoutScratch.intrinsics[node]` for `(axis, self)`.
    /// Encoding lives next to the variant set so adding a `LenReq`
    /// surfaces here, not in `mod.rs`.
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
