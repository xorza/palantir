//! The basis tag a curve instance carries to the shader.

use bytemuck::{Pod, Zeroable};

/// How the curve shader reads a [`CurveInstance`]'s geometry lanes; `repr(transparent)` over the `u32` vertex attribute.
///
/// [`CurveInstance`]: crate::renderer::render_buffer::curve::CurveInstance
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CurveKind(u32);

impl CurveKind {
    pub(crate) const CUBIC: Self = Self(0);
    pub(crate) const ARC: Self = Self(1);
    pub(crate) const SEGMENT: Self = Self(2);
    /// Joint chrome billboards (the three `LineJoin` looks), above every basis kind: `vs` takes the billboard path on `kind >= KIND_JOIN_ROUND`.
    pub(crate) const JOIN_ROUND: Self = Self(3);
    pub(crate) const JOIN_BEVEL: Self = Self(4);
    pub(crate) const JOIN_MITER: Self = Self(5);

    pub(crate) const fn bits(self) -> u32 {
        self.0
    }
}

// The shader splits on `kind >= KIND_JOIN_ROUND`: every basis below it,
// every join at or above it.
const _: () = assert!(
    CurveKind::CUBIC.0 < CurveKind::JOIN_ROUND.0
        && CurveKind::ARC.0 < CurveKind::JOIN_ROUND.0
        && CurveKind::SEGMENT.0 < CurveKind::JOIN_ROUND.0
        && CurveKind::JOIN_BEVEL.0 >= CurveKind::JOIN_ROUND.0
        && CurveKind::JOIN_MITER.0 >= CurveKind::JOIN_ROUND.0
);
