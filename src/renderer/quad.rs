//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use bytemuck::{Pod, Zeroable};

/// Brush-kind tag values packed into `Quad::fill_kind`'s low byte.
/// **Must stay in sync with the `BRUSH_KIND_*` constants in
/// `quad.wgsl`** — the composer writes these, the shader reads them,
/// no other mechanism gates the mapping. (WGSL doesn't reflect at
/// compile time; the agreement is by-eye + the slice-2 visual goldens
/// in step 5.) `Brush::{Solid, Linear}` discriminants are deliberately
/// **not** the source of truth: the composer maps Rust variants → these
/// tags explicitly so reordering the enum can't desync the GPU.
#[allow(dead_code)] // wired in slice-2 step 4 (composer)
pub(crate) const BRUSH_KIND_SOLID: u32 = 0;
#[allow(dead_code)] // wired in slice-2 step 4 (composer)
pub(crate) const BRUSH_KIND_LINEAR: u32 = 1;

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
    /// Brush kind for `fill`. Low byte: 0 = solid, 1 = linear gradient.
    /// Bits 8..16: spread mode (0 = Pad, 1 = Repeat, 2 = Reflect) when
    /// kind == 1; ignored when kind == 0.
    pub(crate) fill_kind: u32,
    /// Row index into the gradient atlas texture when
    /// `fill_kind & 0xFF == 1`. Row 0 is the magenta debug fallback
    /// (any non-zero value here from a misuse paints brightly wrong).
    pub(crate) fill_lut_row: u32,
    /// `(dir_x, dir_y, t0, t1)` — gradient axis direction in
    /// object-local 0..1 space, and the parametric `t` range mapped
    /// across that axis. Ignored when `fill_kind == 0`.
    pub(crate) fill_axis: [f32; 4],
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
