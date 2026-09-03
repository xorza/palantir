//! The `v` two widgets draw as an arrow, as the points a polyline
//! needs.

use glam::Vec2;

/// A chevron in a box of `size`, with the origin at the box's top-left.
///
/// The shape one place rather than two: [`crate::ComboBoxTheme`] draws it
/// pointing down under a dropdown, and [`crate::ExpanderTheme`] turns it
/// from a closed section's `>` to an open one's `v`. They differ in the
/// size they draw it at, never in the shape.
///
/// A polyline rather than a glyph, so it stays font-independent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Chevron {
    pub(crate) size: Vec2,
}

impl Chevron {
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
}
