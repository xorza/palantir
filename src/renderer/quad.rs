//! Per-instance quad data: the Pod type the composer hands, through `RenderBuffer`, to the backend's `QuadPipeline`.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;

use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use bytemuck::{Pod, Zeroable};

/// Per-instance quad data, read through `QUAD_INSTANCE_ATTRS` (backend), which takes each field's offset; no tail padding, as vertex strides need only 4-byte alignment.
///
/// **Solid fill:** `fill_kind` is [`FillKind::SOLID`], `fill` is the color, `fill_lut_row` / `fill_axis` ignored.
///
/// **Linear-gradient fill:** `fill_kind` is [`FillKind::linear`], `fill_lut_row` indexes the gradient atlas row, `fill_axis = (dir_x, dir_y, t0, t1)` gives the projection axis and range; `fill` is white, the shader's multiplier on the ramp, so its alpha carries a fade.
///
/// **Border** is inline `border_color` + `border_width` so the user `Border` can carry non-`Pod` sources; gradient borders are unsupported.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct Quad {
    /// The shape, its turn and its rounded clips, as every template pipeline's instance opens.
    pub(crate) header: ShapeHeader,
    /// Linear-RGB fill as four `f16` (8 B), straight-alpha; the shader premultiplies at output.
    pub(crate) fill: RgbaF16,
    pub(crate) border_color: RgbaF16,
    pub(crate) border_width: f32,
    /// Packed brush metadata; see [`FillKind`] for layout.
    pub(crate) fill_kind: FillKind,
    /// Gradient atlas row when `fill_kind` is a gradient tag; `LutRow::FALLBACK` (0) is the magenta debug fallback, written by solid quads and ignored.
    pub(crate) fill_lut_row: LutRow,
    pub(crate) fill_axis: FillAxis,
}

impl Quad {
    /// A box over `rect`, every other lane at its default.
    pub(crate) fn of_rect(rect: Rect) -> Self {
        Self {
            header: ShapeHeader::of_box(rect),
            ..Self::default()
        }
    }
}
