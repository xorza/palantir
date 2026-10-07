//! NaN screen for authoring inputs.
//!
//! A NaN reaching paint poisons bboxes, cull and damage, and the frame comes out missing rather than broken. [`NanCheck`] stops it at two gates, each where the inputs are still in hand:
//!
//!
//! - **Shapes.** `Shapes::add` refuses to record an authored shape carrying a NaN.
//! - **Chrome.** `lower::background` sanitizes each `Background` field rather than dropping the row, which a rounded clip's stencil mask needs.
//!
//! Impls are `O(1)`, so both gates run in release too. Bulk inputs (polyline points, mesh vertices) are covered by their `bbox` under the AABB NaN contract ([`Aabb`](crate::primitives::geometry::rect::aabb::Aabb)).

use glam::Vec2;

/// True if any scalar the value carries is NaN.
pub(crate) trait NanCheck {
    fn has_nan(&self) -> bool;
}

/// [`NanCheck`] for a `Vec2`, callable from a `const fn` (`NanCheck` cannot be a const trait on stable).
#[inline]
pub(crate) const fn vec2_has_nan(v: Vec2) -> bool {
    v.x.is_nan() || v.y.is_nan()
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
