//! [`ShapeForm`]: the closed-shape half of every closed family's pipeline key.

use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::widget::Coverage;

/// How a closed draw lays its shape out for the vertex stage, pinned as a constant in each closed
/// family's pipeline: a rect's one quad, or the shape template (`shape_template.wgsl`) of a box,
/// a box that covers its outside, or a triangle, so the vertex stage folds the corner count and
/// the side. As Graphite's `RenderStep` follows the geometry
/// ([`Renderer.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/Renderer.h)).
/// Refused: the form as bits of an instance's paint word, read back at upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ShapeForm {
    /// A square-cornered rect's one quad: a plain image.
    Rect = 0,
    /// A box's template, inside.
    Box = 1,
    /// A box's template, painted outside its rounded rect out to the rect.
    BoxOutside = 2,
    /// A triangle's template.
    Triangle = 3,
}

impl ShapeForm {
    /// Every form, in discriminant order.
    pub(crate) const ALL: [Self; 4] = [Self::Rect, Self::Box, Self::BoxOutside, Self::Triangle];

    /// The template form of `geometry`: its kind, and for a box its side.
    pub(crate) const fn of(geometry: &PlacedClosed) -> Self {
        match geometry {
            PlacedClosed::Box {
                coverage: Coverage::Inside,
                ..
            } => Self::Box,
            PlacedClosed::Box {
                coverage: Coverage::Outside,
                ..
            } => Self::BoxOutside,
            PlacedClosed::Triangle(_) => Self::Triangle,
        }
    }

    /// The corners of the form's polygon, which its template draws a ring and a band each of.
    pub(crate) const fn corner_count(self) -> u32 {
        match self {
            Self::Rect | Self::Box | Self::BoxOutside => 4,
            Self::Triangle => 3,
        }
    }

    /// Whether the form paints outside its shape, out to its rect.
    pub(crate) const fn is_outside(self) -> bool {
        matches!(self, Self::BoxOutside)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::shape::geometry::box_geometry::PlacedBox;
    use crate::shape::geometry::closed_geometry::PlacedClosed;
    use crate::shape::geometry::triangle_geometry::PlacedTriangle;
    use crate::widget::Coverage;
    use glam::Vec2;

    /// A box takes its side's form and a triangle its own; only a triangle has three corners and
    /// only an outside box paints outside.
    #[test]
    fn a_shape_takes_its_kinds_and_sides_form() {
        let placed = PlacedBox {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            corners: Corners::ZERO,
        };
        let triangle = PlacedClosed::Triangle(PlacedTriangle {
            centers: [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)],
            radius: 0.0,
        });
        for (geometry, form) in [
            (
                PlacedClosed::Box {
                    placed,
                    coverage: Coverage::Inside,
                },
                ShapeForm::Box,
            ),
            (
                PlacedClosed::Box {
                    placed,
                    coverage: Coverage::Outside,
                },
                ShapeForm::BoxOutside,
            ),
            (triangle, ShapeForm::Triangle),
        ] {
            assert_eq!(ShapeForm::of(&geometry), form, "{geometry:?}");
        }
        assert_eq!(ShapeForm::ALL.map(ShapeForm::corner_count), [4, 4, 4, 3]);
        assert_eq!(
            ShapeForm::ALL.map(ShapeForm::is_outside),
            [false, false, true, false]
        );
    }
}
