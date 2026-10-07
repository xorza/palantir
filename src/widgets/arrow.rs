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

    /// [`Self::points`] turned `radians` about the box's centre. Only square in a square box: a quarter turn swaps extents, clipping one axis.
    pub(crate) fn rotated(self, radians: f32) -> [Vec2; 3] {
        let centre = self.size * 0.5;
        let (sin, cos) = radians.sin_cos();
        self.points().map(|p| {
            let Vec2 { x, y } = p - centre;
            centre + Vec2::new(x * cos - y * sin, x * sin + y * cos)
        })
    }

    /// [`Self::rotated`] as a filled triangle's vertices with corners rounded by `radius`. The renderer rounds by dilating, so vertices sit one radius inside the box to fill it exactly. A radius over half the shorter side is fitted to it.
    pub(crate) fn rounded(self, radius: f32, radians: f32) -> [Vec2; 3] {
        let radius = radius.min(0.5 * self.size.min_element());
        let inset = Vec2::splat(radius);
        Self {
            size: self.size - 2.0 * inset,
        }
        .rotated(radians)
        .map(|p| p + inset)
    }
}
