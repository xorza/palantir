//! The quad stream's rows: each instance beside the key of the pipeline it draws through.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::antialias::{self, AA_HALF_WIDTH};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use glam::Vec2;
use soa_rs::Soars;

/// One quad of the stream. SoA storage lets the backend upload `rows.instance()` in one
/// `write_buffer`, and read `rows.key()` for each instance's pipeline.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct QuadRow {
    /// The pipeline the instance draws through, set once by the composer.
    pub(crate) key: QuadKey,
    /// A fill's [`Quad`], or a shadow's [`ShadowQuad`](crate::renderer::shadow_quad::ShadowQuad)
    /// in a quad's bytes, as `key` says.
    pub(crate) instance: Quad,
}

/// What a quad of the stream draws: a fill through its form, or a shadow, whose entry the
/// backend picks from the cutout plan it alone holds. As `ImageDrawRow::form` keys an image.
/// Refused: the key as bits of the instance's paint word, which the backend read back at upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuadKey {
    Fill(FillForm),
    Shadow,
}

impl QuadKey {
    /// The whole pixels the quad shader shades for a quad of `header`: `shaded_bounds` in
    /// `quad_pipeline/quad.wgsl` grows the rect to every pixel center within [`AA_HALF_WIDTH`]; an
    /// outside rect is drawn at its rect. A turned quad, outside or not, grows [`AA_HALF_WIDTH`]
    /// past each edge in its own frame, and shades within the whole pixels around that turned
    /// about the rect's center.
    pub(crate) fn shaded_rect(self, header: &ShapeHeader) -> Rect {
        let ShapeHeader { rect, rotation, .. } = *header;
        if rotation == 0.0 {
            if self.is_outside() {
                return rect;
            }
            let grow = Vec2::splat(AA_HALF_WIDTH - 0.5);
            return Rect::from_min_max((rect.min - grow).floor(), (rect.max() + grow).ceil());
        }
        let turned = antialias::turned_reach(rect, rotation);
        Rect::from_min_max(turned.min.floor(), turned.max().ceil())
    }

    /// Whether a quad of `header` holds the shape its pipeline draws: a box's points and inradius
    /// are its rect's, as the template builds a box from them; a triangle holds its own, and a
    /// shadow draws no template.
    pub(crate) fn holds_shape(self, header: &ShapeHeader) -> bool {
        match self {
            Self::Fill(form) => form.shape() == ShapeForm::Triangle || header.is_box(),
            Self::Shadow => true,
        }
    }

    const fn is_outside(self) -> bool {
        match self {
            Self::Fill(form) => form.shape().is_outside(),
            Self::Shadow => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
    use crate::renderer::render_buffer::quad_row::QuadKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::renderer::render_buffer::shape_header::ShapeHeader;

    /// A rect on pixel boundaries shades itself; otherwise every pixel its edges cross:
    /// (10.25, 3)..(20.5, 8) is columns 10..21, rows 3..8. An outside rect and a shadow differ:
    /// the outside rect is drawn at its rect, the shadow grows as a fill does.
    #[test]
    fn a_quad_shades_the_pixels_its_edges_cross() {
        let fill = QuadKey::Fill(FillForm::BOX_SOLID);
        let outside = QuadKey::Fill(FillForm::new(ShapeForm::BoxOutside, FillBrush::Gradient));
        let shaded = |key: QuadKey, rect: Rect| {
            key.shaded_rect(&ShapeHeader {
                rect,
                ..ShapeHeader::default()
            })
        };
        let aligned = Rect::new(10.0, 3.0, 11.0, 5.0);
        let off_grid = Rect::new(10.25, 3.0, 10.25, 5.0);
        assert_eq!(shaded(fill, aligned), aligned);
        assert_eq!(shaded(fill, off_grid), aligned);
        assert_eq!(shaded(QuadKey::Shadow, off_grid), aligned);
        assert_eq!(shaded(outside, off_grid), off_grid);
    }

    /// A turned quad shades the whole pixels around its turned bound, grown by the AA half width in
    /// its own frame even on whole-pixel edges. (10, 10)..(30, 20) grown 0.5 is 21 x 11, which
    /// turned 30° about its center (20, 15) reaches `10.5·cos + 5.5·sin = 11.84` across and
    /// `10.5·sin + 5.5·cos = 10.01` down, so (8.16, 4.99)..(31.84, 25.01), out to whole pixels
    /// (8, 4)..(32, 26). Without the growth it would be (8, 5)..(32, 25). Turned, an outside rect
    /// grows too.
    #[test]
    fn a_turned_quad_shades_the_pixels_around_its_turned_bound() {
        let header = ShapeHeader {
            rect: Rect::new(10.0, 10.0, 20.0, 10.0),
            rotation: std::f32::consts::FRAC_PI_6,
            ..ShapeHeader::default()
        };
        let outside = QuadKey::Fill(FillForm::new(ShapeForm::BoxOutside, FillBrush::Solid));
        for key in [QuadKey::Fill(FillForm::BOX_SOLID), outside] {
            assert_eq!(
                key.shaded_rect(&header),
                Rect::new(8.0, 4.0, 24.0, 22.0),
                "{key:?}"
            );
        }
    }

    /// A box fill holds its shape where its points and inradius are its rect's; a triangle and a
    /// shadow hold theirs whatever the header's points.
    #[test]
    fn a_quad_holds_its_shape_by_its_key() {
        let rect = Rect::new(0.0, 0.0, 4.0, 2.0);
        let boxed = ShapeHeader::of_box(rect);
        let bare = ShapeHeader {
            rect,
            ..ShapeHeader::default()
        };
        let fill = QuadKey::Fill(FillForm::BOX_SOLID);
        let triangle = QuadKey::Fill(FillForm::new(ShapeForm::Triangle, FillBrush::Solid));
        assert!(fill.holds_shape(&boxed));
        assert!(!fill.holds_shape(&bare));
        assert!(triangle.holds_shape(&bare));
        assert!(QuadKey::Shadow.holds_shape(&bare));
    }
}
