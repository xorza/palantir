//! The quad stream's rows: each instance beside the key of the pipeline it draws through, and the
//! instance the composer hands, through `RenderBuffer`, to the backend's `QuadPipeline`.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use bytemuck::{Pod, Zeroable};
use soa_rs::Soars;

/// One quad of the stream. SoA storage lets the backend upload `rows.instance()` in one
/// `write_buffer`, and read `rows.key()` for each instance's pipeline.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct QuadDrawRow {
    /// The pipeline the instance draws through, set once by the composer.
    pub(crate) key: QuadKey,
    /// A fill's [`QuadInstance`], or a shadow's [`ShadowInstance`](crate::renderer::render_buffer::shadow_instance::ShadowInstance)
    /// in a quad's bytes, as `key` says.
    pub(crate) instance: QuadInstance,
}

/// What a quad of the stream draws: a fill through its form, or a shadow, whose entry the
/// backend picks from the cutout plan it alone holds. As
/// [`ImageDrawRow::key`](crate::renderer::render_buffer::image_draw_row::ImageDrawRow::key) keys an image.
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

/// Per-instance quad data, read through `QUAD_INSTANCE_ATTRS` (backend), which takes each field's offset; no tail padding, as vertex strides need only 4-byte alignment.
///
/// **Solid fill:** `fill_kind` is [`FillKind::SOLID`], `fill` is the color, `fill_lut_row` / `fill_axis` ignored.
///
/// **Linear-gradient fill:** `fill_kind` is [`FillKind::linear`], `fill_lut_row` indexes the gradient row, `fill_axis = (dir_x, dir_y, t0, t1)` gives the projection axis and range; `fill` is white, the shader's multiplier on the ramp, so its alpha carries a fade.
///
/// **Border** is inline `border_color` + `border_width` so the user `Border` can carry non-`Pod` sources; gradient borders are unsupported.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct QuadInstance {
    /// The shape, its turn and its rounded clips, as every template pipeline's instance opens.
    pub(crate) header: ShapeHeader,
    /// Linear-RGB fill as four `f16` (8 B), straight-alpha; the shader premultiplies at output.
    pub(crate) fill: RgbaF16,
    pub(crate) border_color: RgbaF16,
    pub(crate) border_width: f32,
    /// Packed brush metadata; see [`FillKind`] for layout.
    pub(crate) fill_kind: FillKind,
    /// Gradient row when `fill_kind` is a gradient tag; `LutRow::FALLBACK` (0) is the magenta debug fallback, written by solid quads and ignored.
    pub(crate) fill_lut_row: LutRow,
    pub(crate) fill_axis: FillAxis,
}

impl QuadInstance {
    /// A box over `rect`, every other lane at its default.
    pub(crate) fn of_rect(rect: Rect) -> Self {
        Self {
            header: ShapeHeader::of_box(rect),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
    use crate::renderer::render_buffer::quad_draw_row::QuadKey;
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
