//! The `v` arrow as three points a polyline strokes or a triangle fills.

use glam::Vec2;

/// An arrow in a box of `size`, origin top-left. [`crate::ComboBoxTheme`] strokes it as a chevron, [`crate::ExpanderTheme`] fills it as a triangle; font-independent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrow {
    pub(crate) size: Vec2,
}

impl Arrow {
    /// The three points in box-local pixels; the middle one is the tip at the bottom edge.
    pub(crate) fn points(self) -> [Vec2; 3] {
        let Vec2 { x: w, y: h } = self.size;
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(w * 0.5, h),
            Vec2::new(w, 0.0),
        ]
    }

    /// [`Self::points`] turned `radians` about the box's center. Only square in a square box: a quarter turn swaps extents, clipping one axis.
    pub(crate) fn rotated(self, radians: f32) -> [Vec2; 3] {
        let center = self.size * 0.5;
        let turn = Vec2::from_angle(radians);
        self.points().map(|p| center + turn.rotate(p - center))
    }
}
