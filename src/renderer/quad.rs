//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::brush::{FillAxis, Spread};
use crate::primitives::paint::{FillKind, LutRow};
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect};
use bytemuck::{Pod, Zeroable};

// Compile-time pins for the shader↔CPU discriminant contract. Mirrors
// `BRUSH_KIND_*` / `SPREAD_*` in `src/renderer/backend/quad.wgsl`.
// Reordering `Spread` or the `FillKind` constructors without updating
// the WGSL constants silently mis-renders; bumping a value here trips
// the compile rather than waiting for the runtime tests below or the
// visual goldens.
const _: () = assert!(FillKind::SOLID.0 == 0, "BRUSH_KIND_SOLID");
const _: () = assert!(
    FillKind::linear(Spread::Pad).0 & 0xFF == 1,
    "BRUSH_KIND_LINEAR"
);
const _: () = assert!(
    FillKind::radial(Spread::Pad).0 & 0xFF == 2,
    "BRUSH_KIND_RADIAL"
);
const _: () = assert!(
    FillKind::conic(Spread::Pad).0 & 0xFF == 3,
    "BRUSH_KIND_CONIC"
);
const _: () = assert!(FillKind::SHADOW_DROP.0 == 4, "BRUSH_KIND_SHADOW_DROP");
const _: () = assert!(FillKind::SHADOW_INSET.0 == 5, "BRUSH_KIND_SHADOW_INSET");
const _: () = assert!(FillKind::TRIANGLE.0 == 6, "BRUSH_KIND_TRIANGLE");
const _: () = assert!(FillKind::SOLID.with_fast().0 == 0x10000, "FILL_FLAG_FAST");
const _: () = assert!(
    FillKind::SOLID.with_window().0 == 0x20000,
    "FILL_FLAG_WINDOW"
);
const _: () = assert!(
    (FillKind::linear(Spread::Pad).0 >> 8) & 0xFF == 0,
    "SPREAD_PAD"
);
const _: () = assert!(
    (FillKind::linear(Spread::Repeat).0 >> 8) & 0xFF == 1,
    "SPREAD_REPEAT"
);
const _: () = assert!(
    (FillKind::linear(Spread::Reflect).0 >> 8) & 0xFF == 2,
    "SPREAD_REFLECT"
);

/// Per-instance quad data (60 B). Field types are the matching
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
/// projection axis and parametric range. `fill` is unused (set to zero
/// by the composer).
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
    /// Linear-RGB fill, packed as four `f16` (8 B). Straight-alpha per
    /// the colour-pipeline contract — the shader premultiplies at
    /// output. Halves the 16 B a full `Color` would cost per instance
    /// while keeping enough precision for linear blending.
    pub(crate) fill: ColorF16,
    pub(crate) corners: Corners,
    pub(crate) stroke_color: ColorF16,
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

#[cfg(test)]
mod tests {
    use crate::renderer::quad::Quad;
    use std::mem::offset_of;

    /// Pin: `Quad` is exactly 60 bytes — pos(8) + size(8) +
    /// fill(8, packed 4xf16) + radius(8, packed 4xf16) +
    /// stroke_color(8, packed 4xf16) + stroke_width(4) + fill_kind(4) +
    /// fill_lut_row(4) + fill_axis(8, packed 4xf16). The
    /// `vertex_attr_array` in the backend's `QuadPipeline::new` assumes
    /// this exact layout via Rust's `repr(C)` field-order rules. A
    /// reorder or an added field that shifts an attribute's offset would
    /// break the shader binding silently — this test catches it.
    #[test]
    fn quad_struct_is_60_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<Quad>(), 60);
    }

    /// Pin every field offset against the `vertex_attr_array!` in
    /// `quad_pipeline.rs` (attribute locations 0..=8). A reorder of
    /// same-sized fields wouldn't change the struct size but would
    /// silently mis-bind the shader; size alone can't catch it.
    #[test]
    fn quad_field_offsets_match_vertex_attr_array() {
        assert_eq!(offset_of!(Quad, rect), 0, "loc 0 (pos) + loc 1 (size)");
        assert_eq!(offset_of!(Quad, fill), 16, "loc 2 (fill, packed)");
        assert_eq!(offset_of!(Quad, corners), 24, "loc 3 (radius, packed)");
        assert_eq!(
            offset_of!(Quad, stroke_color),
            32,
            "loc 4 (stroke.color, packed)"
        );
        assert_eq!(offset_of!(Quad, stroke_width), 40, "loc 5 (stroke.width)");
        assert_eq!(offset_of!(Quad, fill_kind), 44, "loc 6 (fill_kind)");
        assert_eq!(offset_of!(Quad, fill_lut_row), 48, "loc 7 (fill_lut_row)");
        assert_eq!(offset_of!(Quad, fill_axis), 52, "loc 8 (fill_axis)");
    }
}
