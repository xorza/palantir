//! [`ShapeHeader`]: the closed shape every template pipeline's instance opens with.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::paint::antialias::{self, AA_HALF_WIDTH};
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

    /// The whole pixels a quad pipeline shades for a quad of this header, a fill's or a shadow's: `shaded_bounds` in
    /// `quad_pipeline/quad.wgsl` grows the rect to every pixel center within [`AA_HALF_WIDTH`]. A
    /// turned quad grows [`AA_HALF_WIDTH`] past each edge in its own frame, and shades within the
    /// whole pixels around that turned about the rect's center.
    pub(crate) fn shaded_rect(&self) -> Rect {
        let Self { rect, rotation, .. } = *self;
        if rotation == 0.0 {
            let grow = Vec2::splat(AA_HALF_WIDTH - 0.5);
            return Rect::from_min_max((rect.min - grow).floor(), (rect.max() + grow).ceil());
        }
        let turned = antialias::turned_reach(rect, rotation);
        Rect::from_min_max(turned.min.floor(), turned.max().ceil())
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_buffer::shape_header::ShapeHeader;

    /// A rect on pixel boundaries shades itself; otherwise every pixel its edges cross:
    /// (10.25, 3)..(20.5, 8) is columns 10..21, rows 3..8.
    #[test]
    fn a_quad_shades_the_pixels_its_edges_cross() {
        let shaded = |rect: Rect| {
            ShapeHeader {
                rect,
                ..ShapeHeader::default()
            }
            .shaded_rect()
        };
        let aligned = Rect::new(10.0, 3.0, 11.0, 5.0);
        assert_eq!(shaded(aligned), aligned);
        assert_eq!(shaded(Rect::new(10.25, 3.0, 10.25, 5.0)), aligned);
    }

    /// A turned quad shades the whole pixels around its turned bound, grown by the AA half width in
    /// its own frame even on whole-pixel edges. (10, 10)..(30, 20) grown 0.5 is 21 x 11, which
    /// turned 30° about its center (20, 15) reaches `10.5·cos + 5.5·sin = 11.84` across and
    /// `10.5·sin + 5.5·cos = 10.01` down, so (8.16, 4.99)..(31.84, 25.01), out to whole pixels
    /// (8, 4)..(32, 26). Without the growth it would be (8, 5)..(32, 25).
    #[test]
    fn a_turned_quad_shades_the_pixels_around_its_turned_bound() {
        let header = ShapeHeader {
            rect: Rect::new(10.0, 10.0, 20.0, 10.0),
            rotation: std::f32::consts::FRAC_PI_6,
            ..ShapeHeader::default()
        };
        assert_eq!(header.shaded_rect(), Rect::new(8.0, 4.0, 24.0, 22.0));
    }
}
