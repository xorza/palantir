//! Per-axis twins of the [`domain`](super) rules for two-axis widgets: the scalar rule on `x` and `y` each.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use glam::Vec2;

/// True if `a` and `b` are within [`EPS`](domain::EPS) by Euclidean distance; for coincident points (degenerate stroke endpoints, zero-length segments).
#[inline]
pub(crate) const fn approx_eq(a: Vec2, b: Vec2) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy <= domain::EPS * domain::EPS
}

/// [`domain::band_fraction`] per axis.
#[inline]
pub(crate) const fn band_fraction(pos: Vec2, extent: Size, band: Size) -> Vec2 {
    Vec2::new(
        domain::band_fraction(pos.x, extent.w, band.w),
        domain::band_fraction(pos.y, extent.h, band.h),
    )
}

/// [`domain::fraction_or`] per axis.
#[inline]
#[track_caller]
pub(crate) const fn fraction_or(v: Vec2, fallback: Vec2) -> Vec2 {
    Vec2::new(
        domain::fraction_or(v.x, fallback.x),
        domain::fraction_or(v.y, fallback.y),
    )
}

/// True if both axes are [offsets](domain::offset).
#[inline]
pub(crate) const fn is_offset(v: Vec2) -> bool {
    domain::is_offset(v.x) && domain::is_offset(v.y)
}

/// `v`, whose axes must both be *offsets*.
///
/// # Panics
///
/// Panics unless [`is_offset`]`(v)`.
#[inline]
#[track_caller]
pub(crate) const fn offset(v: Vec2) -> Vec2 {
    assert!(is_offset(v), "{}", domain::OFFSET_RULE);
    v
}

/// True if both axes are [lanes](domain::lane).
#[inline]
pub(crate) const fn is_lane(v: Vec2) -> bool {
    domain::is_lane(v.x) && domain::is_lane(v.y)
}

/// `v`, whose axes must both be *lanes*.
///
/// # Panics
///
/// Panics unless [`is_lane`]`(v)`.
#[inline]
#[track_caller]
pub(crate) const fn lane(v: Vec2) -> Vec2 {
    assert!(is_lane(v), "{}", domain::LANE_RULE);
    v
}

/// True if both axes are [file lengths](domain::is_file_length).
#[inline]
pub(crate) const fn is_file_length(v: Vec2) -> bool {
    domain::is_file_length(v.x) && domain::is_file_length(v.y)
}

/// True if both axes are [file offsets](domain::is_file_offset).
#[inline]
pub(crate) const fn is_file_offset(v: Vec2) -> bool {
    domain::is_file_offset(v.x) && domain::is_file_offset(v.y)
}

/// True if both axes are [lengths](domain::length).
#[inline]
pub(crate) const fn is_length(v: Vec2) -> bool {
    domain::is_length(v.x) && domain::is_length(v.y)
}

/// `v`, whose axes must both be *lengths*.
///
/// # Panics
///
/// Panics unless [`is_length`]`(v)`.
#[inline]
#[track_caller]
pub(crate) const fn length(v: Vec2) -> Vec2 {
    assert!(is_length(v), "{}", domain::LENGTH_RULE);
    v
}
