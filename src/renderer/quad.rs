//! Per-instance quad data: the Pod type the composer hands, through `RenderBuffer`, to the backend's `QuadPipeline`.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// Per-instance quad data (64 B). Field order is constrained by `QUAD_INSTANCE_ATTRS` (backend); no tail padding, as vertex strides need only 4-byte alignment.
///
/// **Solid fill:** `fill_kind` is [`FillKind::SOLID`], `fill` is the color, `fill_lut_row` / `fill_axis` ignored.
///
/// **Linear-gradient fill:** `fill_kind` is [`FillKind::linear`], `fill_lut_row` indexes the gradient atlas row, `fill_axis = (dir_x, dir_y, t0, t1)` gives the projection axis and range; `fill` is white, the shader's multiplier on the ramp, so its alpha carries a fade.
///
/// **Stroke** is inline `stroke_color` + `stroke_width` so the user `Stroke` can carry non-`Pod` sources; gradient strokes are unsupported.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct Quad {
    pub(crate) rect: Rect,
    /// Linear-RGB fill as four `f16` (8 B), straight-alpha; the shader premultiplies at output.
    pub(crate) fill: RgbaF16,
    pub(crate) corners: Corners,
    pub(crate) stroke_color: RgbaF16,
    pub(crate) stroke_width: f32,
    /// Packed brush metadata; see [`FillKind`] for layout.
    pub(crate) fill_kind: FillKind,
    /// Gradient atlas row when `fill_kind` is a gradient tag; `LutRow::FALLBACK` (0) is the magenta debug fallback, written by solid quads and ignored.
    pub(crate) fill_lut_row: LutRow,
    pub(crate) fill_axis: FillAxis,
    /// A paint animation's turn about the rect's center, in radians; `0` for none. `rect` and
    /// every lane above stay in the quad's own unturned frame.
    pub(crate) rotation: f32,
}

impl Quad {
    /// The whole pixels the quad shader shades: `shaded_bounds` in `quad_pipeline/shader.wgsl` grows the rect to every pixel center within [`AA_HALF_WIDTH`]; a windowed rect is drawn at its rect. A turned quad, windowed or not, grows [`AA_HALF_WIDTH`] past each edge in its own frame, and shades within the whole pixels around that turned about the rect's center.
    pub(crate) fn shaded_rect(&self) -> Rect {
        if self.rotation == 0.0 {
            if self.fill_kind.is_window() {
                return self.rect;
            }
            let grow = Vec2::splat(AA_HALF_WIDTH - 0.5);
            return Rect::from_min_max(
                (self.rect.min - grow).floor(),
                (self.rect.max() + grow).ceil(),
            );
        }
        let turned = self
            .rect
            .inflated(AA_HALF_WIDTH)
            .turned_cover(self.rect.center(), self.rotation);
        Rect::from_min_max(turned.min.floor(), turned.max().ceil())
    }
}

// Layout guards live where it is consumed: `offset_of!` asserts beside `QUAD_INSTANCE_ATTRS` and `hot_struct_sizes_are_pinned` in `lib.rs`.

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::renderer::quad::Quad;

    /// A rect on pixel boundaries shades itself; otherwise every pixel its edges cross: (10.25, 3)..(20.5, 8) is columns 10..21, rows 3..8.
    #[test]
    fn a_quad_shades_the_pixels_its_edges_cross() {
        let shaded = |rect: Rect, fill_kind: FillKind| {
            Quad {
                rect,
                fill_kind,
                ..Quad::default()
            }
            .shaded_rect()
        };
        let aligned = Rect::new(10.0, 3.0, 11.0, 5.0);
        let off_grid = Rect::new(10.25, 3.0, 10.25, 5.0);
        assert_eq!(shaded(aligned, FillKind::SOLID), aligned);
        assert_eq!(shaded(off_grid, FillKind::SOLID), aligned);
        assert_eq!(shaded(off_grid, FillKind::SOLID.with_window()), off_grid);
    }

    /// A turned quad shades the whole pixels around its turned bound, grown by the AA half width in
    /// its own frame even on whole-pixel edges. (10, 10)..(30, 20) grown 0.5 is 21 x 11, which
    /// turned 30° about its center (20, 15) reaches `10.5·cos + 5.5·sin = 11.84` across and
    /// `10.5·sin + 5.5·cos = 10.01` down, so (8.16, 4.99)..(31.84, 25.01), out to whole pixels
    /// (8, 4)..(32, 26). Without the growth it would be (8, 5)..(32, 25).
    #[test]
    fn a_turned_quad_shades_the_pixels_around_its_turned_bound() {
        let quad = Quad {
            rect: Rect::new(10.0, 10.0, 20.0, 10.0),
            fill_kind: FillKind::SOLID,
            rotation: std::f32::consts::FRAC_PI_6,
            ..Quad::default()
        };
        assert_eq!(quad.shaded_rect(), Rect::new(8.0, 4.0, 24.0, 22.0));
    }
}
