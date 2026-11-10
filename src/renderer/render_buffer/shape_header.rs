//! [`ShapeHeader`]: the closed shape every template pipeline's instance opens with.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// A closed shape in physical px, as the shape template takes it, at the head of a quad's, an
/// image's and an effect's instance, so each pipeline reads it at the same attribute locations
/// through one WGSL reader (`PalantirShapeIn`, `palantir_shape_of`). Refused: the shape at other
/// offsets in each instance, three layouts and three readers that must agree by hand.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ShapeHeader {
    /// The shape's covering rect, unturned, moved by a turn about its pivot.
    pub(crate) rect: Rect,
    /// The sharp points from `rect.min`, clockwise on screen: a box's `(tl, tr, br, bl)`, a
    /// triangle's three points grown by its radius, its fourth unused. With `corners`' radii, what
    /// the template's vertex stage builds the shape from (`shape_template.wgsl`), with no branch
    /// per kind of shape.
    pub(crate) points: [Vec2; 4],
    /// A radius at each point, physical px.
    pub(crate) corners: Corners,
    /// The radius of the largest circle inside the sharp polygon, the deepest any point lies inside
    /// every edge: half a box's shorter side, a triangle's own inradius plus its growth.
    pub(crate) inradius: f32,
    /// A paint animation's turn about `rect`'s center, in radians; `0` for none. `rect` and every
    /// lane stay in the shape's own unturned frame.
    pub(crate) rotation: f32,
    /// The rounded clips that cut the shape; [`ClipChain::NONE`] by default.
    pub(crate) clip: ClipChain,
}

impl ShapeHeader {
    /// A sharp box over `rect`: its points the rect's corners and its inradius half its shorter
    /// side, unturned and uncut.
    pub(crate) fn of_box(rect: Rect) -> Self {
        Self {
            rect,
            points: Self::box_points(rect.size),
            inradius: Self::box_inradius(rect.size),
            ..Self::default()
        }
    }

    /// The corners of a box of `size` from its top-left, `(tl, tr, br, bl)`.
    pub(crate) const fn box_points(size: Size) -> [Vec2; 4] {
        [
            Vec2::ZERO,
            Vec2::new(size.w, 0.0),
            Vec2::new(size.w, size.h),
            Vec2::new(0.0, size.h),
        ]
    }

    /// The inradius of a box of `size`: half its shorter side.
    pub(crate) const fn box_inradius(size: Size) -> f32 {
        0.5 * size.w.min(size.h)
    }

    /// Whether the header holds the box its rect makes, as the template builds a box from its
    /// points and inradius.
    pub(crate) fn is_box(&self) -> bool {
        self.points == Self::box_points(self.rect.size)
            && self.inradius == Self::box_inradius(self.rect.size)
    }
}
