//! Debug-only NaN screen for authoring inputs.
//!
//! A NaN that reaches the paint pipeline is a caller bug, and a quiet
//! one. It survives every arithmetic hop, poisons whatever bbox it lands
//! in, and from there the cull rect, the damage region, and everything
//! those union with — while the shader's `> 0.0` comparisons all read
//! false and paint nothing. `f32::max` even launders it back to a finite
//! number at some hops, so the corruption doesn't always reach a place
//! that would look wrong. The frame comes out *missing*, not broken:
//! nothing to see, nothing to bisect.
//!
//! [`NanCheck`] is what stops it, at the one gate that decides it:
//! `Shapes::add` tests the lowered record and refuses to record a shape
//! that carries a NaN, loudly in debug and quietly in release.
//!
//! Every impl is `O(1)`, which is what lets the gate run in release as
//! well as debug — so a NaN is *dropped* in a shipped build, not just
//! reported in a dev one. Bulk inputs (polyline points, mesh vertices,
//! curve control points) get there by having been folded into a `bbox`
//! under the AABB NaN contract (see
//! [`Aabb`](crate::primitives::rect::aabb::Aabb)) — one `Rect` test
//! stands in for scanning the data behind it.
//!
//! `f32` is the leaf every other impl bottoms out in.

use glam::Vec2;

/// True if any scalar the value carries is NaN. See the module doc for
/// where this is checked.
pub(crate) trait NanCheck {
    fn has_nan(&self) -> bool;
}

/// [`NanCheck`] for a `Vec2`, callable from a `const fn`.
///
/// Exists because neither spelling works in a const context: `NanCheck`
/// cannot be a const trait on stable, and glam's `Vec2::is_nan` is not
/// `const` either. `f32::is_nan` *is*, so the const paint predicates
/// (`Rect::is_paint_empty`, `Rect::from_min_max`, `Shadow::is_noop`)
/// route through this rather than each open-coding the two lanes.
#[inline]
pub(crate) const fn vec2_has_nan(v: Vec2) -> bool {
    v.x.is_nan() || v.y.is_nan()
}

impl NanCheck for f32 {
    #[inline]
    fn has_nan(&self) -> bool {
        self.is_nan()
    }
}

impl NanCheck for Vec2 {
    #[inline]
    fn has_nan(&self) -> bool {
        vec2_has_nan(*self)
    }
}

/// `None` carries no scalar, so it has nothing to be NaN.
impl<T: NanCheck> NanCheck for Option<T> {
    #[inline]
    fn has_nan(&self) -> bool {
        self.as_ref().is_some_and(NanCheck::has_nan)
    }
}

impl<T: NanCheck> NanCheck for [T] {
    #[inline]
    fn has_nan(&self) -> bool {
        self.iter().any(NanCheck::has_nan)
    }
}
