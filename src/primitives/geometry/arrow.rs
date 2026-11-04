//! The `v` arrow in a box of a [`Size`], origin top-left, as three points a polyline strokes or a
//! triangle fills: [`crate::ComboBoxTheme`] strokes it as a chevron, [`crate::ExpanderTheme`] fills
//! it as a triangle; font-independent.

use crate::primitives::geometry::size::Size;
use glam::Vec2;

/// The three points in box-local pixels; the middle one is the tip at the bottom edge.
pub(crate) const fn points(size: Size) -> [Vec2; 3] {
    [
        Vec2::new(0.0, 0.0),
        Vec2::new(size.w * 0.5, size.h),
        Vec2::new(size.w, 0.0),
    ]
}

/// [`points`] turned `radians` about the box's center. Only square in a square box: a quarter turn swaps extents, clipping one axis.
pub(crate) fn rotated(size: Size, radians: f32) -> [Vec2; 3] {
    let center = Vec2::from(size) * 0.5;
    let turn = Vec2::from_angle(radians);
    points(size).map(|p| center + turn.rotate(p - center))
}
