//! The `v` two widgets draw as an arrow: the three points a polyline
//! strokes or a triangle fills.

use glam::Vec2;

/// An arrow in a box of `size`, with the origin at the box's top-left.
///
/// The shape one place rather than two: [`crate::ComboBoxTheme`] strokes
/// it as a chevron pointing down under a dropdown, and
/// [`crate::ExpanderTheme`] fills it as the disclosure triangle it turns
/// from a closed section's `>` to an open one's `v`. They differ in how
/// they paint it, never in the shape.
///
/// Points rather than a glyph, so it stays font-independent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrow {
    pub(crate) size: Vec2,
}

impl Arrow {
    /// The three points, in box-local pixels. The middle one is the tip,
    /// at the bottom edge.
    pub(crate) fn points(self) -> [Vec2; 3] {
        let Vec2 { x: w, y: h } = self.size;
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(w * 0.5, h),
            Vec2::new(w, 0.0),
        ]
    }

    /// [`Self::points`] turned `radians` about the box's centre.
    ///
    /// **Only square in a square box.** A quarter turn swaps the shape's
    /// extents, so a box narrower than it is tall clips the turned arrow
    /// on one axis and leaves a gap on the other.
    pub(crate) fn rotated(self, radians: f32) -> [Vec2; 3] {
        let centre = self.size * 0.5;
        let (sin, cos) = radians.sin_cos();
        self.points().map(|p| {
            let Vec2 { x, y } = p - centre;
            centre + Vec2::new(x * cos - y * sin, x * sin + y * cos)
        })
    }

    /// [`Self::rotated`], as the vertices of a filled triangle whose
    /// corners are rounded by `radius`.
    ///
    /// The renderer rounds a triangle by dilating it by the radius, so
    /// the vertices sit one radius inside the box: the rounded shape
    /// then fills the box exactly, instead of overrunning it on every
    /// side. The turn is about the box's centre either way — the inset
    /// is the same on every side, so the two boxes share it.
    pub(crate) fn rounded(self, radius: f32, radians: f32) -> [Vec2; 3] {
        debug_assert!(
            2.0 * radius <= self.size.min_element(),
            "a corner radius of {radius} does not fit an arrow of {:?}",
            self.size
        );
        let inset = Vec2::splat(radius);
        Self {
            size: self.size - 2.0 * inset,
        }
        .rotated(radians)
        .map(|p| p + inset)
    }
}
