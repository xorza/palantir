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
//! [`NanCheck`] is what makes it loud instead, at the one boundary where
//! the offending value still has a name and a call site — see
//! `Shape::debug_assert_no_nan`, which `Shapes::add` runs beside
//! `Shape::is_noop`.
//!
//! Every impl exists to feed a `debug_assert!`, so release builds pay
//! nothing; the per-shape budget forbids anything else. That is also
//! what makes the two slice-walking impls (polyline points, mesh
//! vertices) affordable — they are `O(n)` in debug only.
//!
//! `f32` is the leaf every other impl bottoms out in.

use glam::Vec2;

/// True if any scalar the value carries is NaN. See the module doc for
/// where this is checked and why it is debug-only.
pub(crate) trait NanCheck {
    fn has_nan(&self) -> bool;
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
        Vec2::is_nan(*self)
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
