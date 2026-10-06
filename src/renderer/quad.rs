//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// Per-instance quad data (60 B). Field types are the matching
/// `repr(C)` primitives, byte-identical to `[f32; N]`s — see
/// `QUAD_INSTANCE_ATTRS` (in the backend) for the explicit attribute
/// offsets, which is the only thing constraining the field order. No tail padding: vertex buffer strides only need
/// 4-byte alignment, unlike std140 uniforms.
///
/// **Solid fill:** `fill_kind` is [`FillKind::SOLID`], `fill` carries the
/// colour, `fill_lut_row` / `fill_axis` ignored.
///
/// **Linear-gradient fill:** `fill_kind` is [`FillKind::linear`], which
/// carries the `Spread` beside the tag, `fill_lut_row` indexes the gradient atlas texture
/// row, `fill_axis = (dir_x, dir_y, t0, t1)` gives the object-space
/// projection axis and parametric range. `fill` is white, the multiplier
/// the shader applies to the ramp's colour (`c * in.fill`), so its alpha
/// carries a fade.
///
/// **Stroke** is stored as inline `stroke_color` + `stroke_width`
/// fields rather than an embedded `Stroke` so the user-facing `Stroke`
/// is free to carry non-`Pod` paint sources (`Brush`); the composer
/// translates the user `Stroke` into these GPU fields. Stroke-as-
/// gradient is not supported.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct Quad {
    pub(crate) rect: Rect,
    /// Linear-RGB fill, packed as four `f16` (8 B). Straight-alpha per
    /// the colour-pipeline contract — the shader premultiplies at
    /// output. Halves the 16 B a full `RgbaF32` would cost per instance
    /// while keeping enough precision for linear blending.
    pub(crate) fill: RgbaF16,
    pub(crate) corners: Corners,
    pub(crate) stroke_color: RgbaF16,
    pub(crate) stroke_width: f32,
    /// Packed brush metadata; see [`FillKind`] for layout.
    pub(crate) fill_kind: FillKind,
    /// Row index into the gradient atlas texture when `fill_kind`'s
    /// low byte is a gradient tag (1..=3). `LutRow(0)`
    /// (`LutRow::FALLBACK`) is the magenta debug fallback — any quad
    /// reaching the sampler with that value paints magenta. Solid
    /// quads write `LutRow::FALLBACK` and the shader ignores the field.
    pub(crate) fill_lut_row: LutRow,
    /// Gradient axis vector — see [`FillAxis`]. Ignored when
    /// `fill_kind == FillKind::SOLID`.
    pub(crate) fill_axis: FillAxis,
}

impl Quad {
    /// The whole pixels the quad shader shades for this quad:
    /// `shaded_bounds` in `quad_pipeline/shader.wgsl` grows the rect to
    /// every pixel centre within [`AA_HALF_WIDTH`] of it, since its
    /// coverage reaches that far. A windowed rect is drawn at its rect.
    pub(crate) fn shaded_rect(&self) -> Rect {
        if self.fill_kind.is_window() {
            return self.rect;
        }
        let grow = Vec2::splat(AA_HALF_WIDTH - 0.5);
        Rect::from_min_max(
            (self.rect.min - grow).floor(),
            (self.rect.max() + grow).ceil(),
        )
    }
}

// Layout guards live where the layout is consumed: the compile-time
// `offset_of!` asserts beside `QUAD_INSTANCE_ATTRS` in
// `gpu/pipeline/quad_pipeline/mod.rs` pin every field against its vertex
// attribute, and the `hot_struct_sizes_are_pinned` inventory in
// `lib.rs` pins the 60/4 footprint.

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::renderer::quad::Quad;

    /// A rect on pixel boundaries shades itself. One off them shades every
    /// pixel its edges cross: (10.25, 3)..(20.5, 8) is columns 10..21 and
    /// rows 3..8. A windowed rect is drawn at its rect.
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
        assert_eq!(shaded(off_grid, FillKind::SOLID.with_window()), off_grid,);
    }
}
