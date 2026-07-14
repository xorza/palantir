//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::brush::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect};
use bytemuck::{Pod, Zeroable};

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

// Layout guards live where the layout is consumed: the compile-time
// `offset_of!` asserts beside `QUAD_INSTANCE_ATTRS` in
// `backend/quad_pipeline.rs` pin every field against its vertex
// attribute, and the `hot_struct_sizes_are_pinned` inventory in
// `lib.rs` pins the 60/4 footprint.
