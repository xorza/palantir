//! The quad stream's rows: each instance beside the key of the pipeline it draws through.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
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
    /// Whether a quad of `header` holds the shape its pipeline draws: a box's points and inradius
    /// are its rect's, as the template builds a box from them; a triangle holds its own, and a
    /// shadow draws no template.
    pub(crate) fn holds_shape(self, header: &ShapeHeader) -> bool {
        match self {
            Self::Fill(form) => form.shape() == ShapeForm::Triangle || header.is_box(),
            Self::Shadow => true,
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
