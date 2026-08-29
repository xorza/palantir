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
//! [`NanCheck`] is what stops it, at the two gates that decide it — one
//! per path a paint input can arrive on, each placed where the inputs are
//! still in hand and nothing has been staged yet:
//!
//! - **Shapes.** `Shapes::add` asks the *authored* shape, before
//!   lowering, and refuses to record one that carries a NaN. Loud in
//!   debug, quiet in release.
//! - **Chrome.** `lower::background` asks the `Background`, and
//!   sanitizes each field to what its NaN already meant rather than
//!   dropping the row — a rounded-clip node keeps a chrome row even when
//!   its paint is fully no-op, so dropping would leave the stencil mask
//!   reading the NaN.
//!
//! Every impl is `O(1)`, which is what lets both gates run in release as
//! well as debug — so a NaN is *dropped* in a shipped build, not just
//! reported in a dev one. Bulk inputs (polyline points, mesh vertices)
//! get there by having been folded into a `bbox` under the AABB NaN
//! contract (see [`Aabb`](crate::primitives::rect::aabb::Aabb)) — one
//! `Rect` test stands in for scanning the data behind it.
//!
//! Scalar lanes bottom out in `f32::is_nan` directly; the trait starts
//! at the composite types, which is where a caller has something worth
//! naming.

use glam::Vec2;

/// True if any scalar the value carries is NaN. See the module doc for
/// where this is checked.
///
/// A type whose `const` predicates also need the sweep — [`Shadow`],
/// [`Color`], [`Rect`], [`Size`] — carries it as an inherent `const fn`
/// and implements this trait by delegating there, so the field walk is
/// written once.
///
/// [`Shadow`]: crate::primitives::shadow::Shadow
/// [`Color`]: crate::primitives::color::Color
/// [`Rect`]: crate::primitives::rect::Rect
/// [`Size`]: crate::primitives::size::Size
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
