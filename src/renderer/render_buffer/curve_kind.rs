//! The basis tag a curve instance carries to the shader.

use bytemuck::{Pod, Zeroable};

/// How the curve shader reads a [`CurveInstance`]'s geometry lanes — see
/// its docs for each kind's lanes. `repr(transparent)` over the `u32`
/// vertex attribute the shader reads, which takes each tag as a
/// substituted constant.
///
/// [`CurveInstance`]: crate::renderer::render_buffer::curve::CurveInstance
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CurveKind(u32);

impl CurveKind {
    pub(crate) const CUBIC: Self = Self(0);
    pub(crate) const ARC: Self = Self(1);
    /// Straight polyline segment with bisector-clipped joint ends.
    pub(crate) const SEGMENT: Self = Self(2);
    /// Joint chrome billboards — the three `LineJoin` looks. They sit above
    /// every basis kind, which is the one thing the shader reads off their
    /// numbering: `kind >= KIND_JOIN_ROUND` is how `vs` takes the billboard
    /// path. Which look to paint rides a flag bit the vertex stage sets by
    /// comparing against each kind, so their order among themselves is free.
    pub(crate) const JOIN_ROUND: Self = Self(3);
    pub(crate) const JOIN_BEVEL: Self = Self(4);
    pub(crate) const JOIN_MITER: Self = Self(5);

    /// The tag as the shader compares it.
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
