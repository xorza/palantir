//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::brush::{FillAxis, Spread};
use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use bytemuck::{Pod, Zeroable};

/// Packed fill-brush metadata for `Quad.fill_kind` and the matching
/// cmd-buffer payload fields. Low byte: kind tag (0 = solid,
/// 1 = linear). Bits 8..16: `Spread` discriminant (only meaningful
/// when kind == linear).
///
/// `repr(transparent)` over `u32` so the GPU wire layout is just a
/// `u32` vertex attribute — `vertex_attr_array![..., 6 => Uint32, ...]`
/// in the pipeline matches the shader's `@location(6) fill_kind: u32`
/// against this wrapper directly.
///
/// **Shader-side mapping** (`quad.wgsl`): the bit-layout constants
/// `BRUSH_KIND_SOLID = 0u` / `BRUSH_KIND_LINEAR = 1u` and the spread
/// tags `0..2` are hand-mirrored. Reordering `Brush` or `Spread`
/// without updating WGSL silently desyncs; the slice-2 visual goldens
/// catch it.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct FillKind(u32);

impl FillKind {
    /// Solid-fill marker; `Quad.fill: Color` carries the colour, the
    /// LUT / axis / row fields are ignored by the shader.
    pub(crate) const SOLID: Self = Self(0);

    /// Linear-gradient marker with the spread mode packed into bits
    /// 8..16. The atlas row id and axis vector ride along in
    /// `Quad.fill_lut_row` / `Quad.fill_axis`.
    pub(crate) const fn linear(spread: Spread) -> Self {
        Self(1 | ((spread as u32) << 8))
    }

    /// `true` when the kind tag is `0`.
    #[allow(dead_code)] // used in tests + future is_solid fast paths
    #[inline]
    pub(crate) const fn is_solid(self) -> bool {
        (self.0 & 0xFF) == 0
    }

    /// `true` when the kind tag is `1`. Used by the composer to decide
    /// whether to register the gradient with the LUT atlas.
    #[inline]
    pub(crate) const fn is_linear(self) -> bool {
        (self.0 & 0xFF) == 1
    }
}

/// Per-instance quad data (92 B). Field types are the matching
/// `repr(C)` primitives, byte-identical to `[f32; N]`s — see the
/// `vertex_attr_array` in `QuadPipeline::new` (in the backend) for the
/// explicit attribute offsets, which is the only thing constraining
/// the field order. No tail padding: vertex buffer strides only need
/// 4-byte alignment, unlike std140 uniforms.
///
/// **Solid fill:** `fill_kind = 0`, `fill: Color` carries the colour,
/// `fill_lut_row` / `fill_axis` ignored.
///
/// **Linear-gradient fill:** `fill_kind` low byte = 1, bits 8..16 carry
/// the `Spread` enum, `fill_lut_row` indexes the gradient atlas texture
/// row, `fill_axis = (dir_x, dir_y, t0, t1)` gives the object-space
/// projection axis and parametric range. `fill: Color` is unused (set
/// to zero by the composer).
///
/// **Stroke** is stored as inline `stroke_color` + `stroke_width`
/// fields rather than an embedded `Stroke` so the user-facing `Stroke`
/// is free to carry non-`Pod` paint sources (`Brush`); the composer
/// translates the user `Stroke` into these GPU fields. Stroke-as-
/// gradient is a slice-2 non-goal (see `docs/roadmap/brushes-slice-2-plan.md`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct Quad {
    pub(crate) rect: Rect,
    pub(crate) fill: Color,
    pub(crate) radius: Corners,
    pub(crate) stroke_color: Color,
    pub(crate) stroke_width: f32,
    /// Packed brush metadata; see [`FillKind`] for layout.
    pub(crate) fill_kind: FillKind,
    /// Row index into the gradient atlas texture when
    /// `fill_kind & 0xFF == 1`. Row 0 is the magenta debug fallback
    /// (any non-zero value here from a misuse paints brightly wrong).
    pub(crate) fill_lut_row: u32,
    /// Gradient axis vector — see [`FillAxis`]. Ignored when
    /// `fill_kind.is_solid()`.
    pub(crate) fill_axis: FillAxis,
}

#[cfg(test)]
mod tests {
    use super::Quad;

    /// Pin: `Quad` is exactly 92 bytes — pos(8) + size(8) + fill(16) +
    /// radius(16) + stroke_color(16) + stroke_width(4) + fill_kind(4) +
    /// fill_lut_row(4) + fill_axis(16). The `vertex_attr_array` in the
    /// backend's `QuadPipeline::new` assumes this exact layout via
    /// Rust's `repr(C)` field-order rules. A reorder or an added field
    /// that shifts an attribute's offset would break the shader binding
    /// silently — this test catches it.
    #[test]
    fn quad_struct_is_92_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<Quad>(), 92);
    }
}
