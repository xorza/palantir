//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::brush::{FillAxis, Spread};
use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use crate::renderer::gradient_atlas::LutRow;
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

    /// Radial-gradient marker. `fill_axis` carries `(cx, cy, rx, ry)`
    /// in object-space 0..1 coords; the shader projects each fragment
    /// onto the elliptical radius to derive `t`.
    pub(crate) const fn radial(spread: Spread) -> Self {
        Self(2 | ((spread as u32) << 8))
    }

    /// Conic-gradient marker. `fill_axis` carries `(cx, cy,
    /// start_angle, _)`; the shader uses `atan2` to derive `t`.
    pub(crate) const fn conic(spread: Spread) -> Self {
        Self(3 | ((spread as u32) << 8))
    }

    /// Drop-shadow marker. `fill: Color` carries the shadow colour,
    /// `fill_axis = (offset.x, offset.y, sigma, spread)`,
    /// `radius` carries the *source* shape's corner radii (the shadow
    /// is paint-bbox-aligned but conceptually wraps a source rect at
    /// `rect_centre - offset`). The shader runs `shadow_coverage` and
    /// multiplies `fill.rgb * fill.a * cov`.
    pub(crate) const SHADOW_DROP: Self = Self(4);

    /// Inset-shadow marker. Same packing as `SHADOW_DROP`; the
    /// shader inverts coverage and clips to inside the source rect.
    pub(crate) const SHADOW_INSET: Self = Self(5);

    /// `true` when the kind tag is `0`.
    #[allow(dead_code)] // used in tests + future is_solid fast paths
    #[inline]
    pub(crate) const fn is_solid(self) -> bool {
        (self.0 & 0xFF) == 0
    }

    /// `true` for any gradient variant. Used by the composer to decide
    /// whether to register the gradient stops with the LUT atlas. All
    /// gradient variants share the same atlas keying (stops + interp);
    /// only the per-fragment `t` derivation differs.
    #[inline]
    pub(crate) const fn is_gradient(self) -> bool {
        matches!(self.0 & 0xFF, 1..=3)
    }

    /// `true` for any shadow variant. Composer scales `fill_axis`
    /// (`offset_xy, sigma, _`) by DPI for these — gradient axes are
    /// 0..1 local coords and stay scale-free, but shadow params are
    /// physical-px in the shader.
    #[inline]
    pub(crate) const fn is_shadow(self) -> bool {
        matches!(self.0 & 0xFF, 4..=5)
    }
}

/// Per-instance quad data (84 B). Field types are the matching
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
    /// `fill_kind & 0xFF == 1`. `LutRow(0)` (`LutRow::FALLBACK`) is the
    /// magenta debug fallback — any quad reaching the sampler with that
    /// value paints magenta. Solid quads write `LutRow::FALLBACK` and
    /// the shader ignores the field for `fill_kind.is_solid()`.
    pub(crate) fill_lut_row: LutRow,
    /// Gradient axis vector — see [`FillAxis`]. Ignored when
    /// `fill_kind.is_solid()`.
    pub(crate) fill_axis: FillAxis,
}

#[cfg(test)]
mod tests {
    use super::{FillKind, Quad};
    use crate::primitives::brush::Spread;
    use std::mem::offset_of;

    /// Pin: brush-kind discriminants match the WGSL `BRUSH_KIND_*`
    /// constants in `src/renderer/backend/quad.wgsl`. Reordering
    /// either side silently desyncs — the shader path picks the
    /// wrong branch, no compile error. Cross-checked at the byte
    /// level because that's what the vertex attribute carries.
    #[test]
    fn fill_kind_discriminants_match_wgsl() {
        // Solid / Linear / Radial / Conic carry the kind in the low
        // byte; Spread rides in bits 8..16 but isn't relevant here.
        assert_eq!(FillKind::SOLID.0 & 0xFF, 0);
        assert_eq!(FillKind::linear(Spread::Pad).0 & 0xFF, 1);
        assert_eq!(FillKind::radial(Spread::Pad).0 & 0xFF, 2);
        assert_eq!(FillKind::conic(Spread::Pad).0 & 0xFF, 3);
        assert_eq!(FillKind::SHADOW_DROP.0 & 0xFF, 4);
        assert_eq!(FillKind::SHADOW_INSET.0 & 0xFF, 5);
        // Sanity: shadow detector covers exactly 4..=5.
        assert!(FillKind::SHADOW_DROP.is_shadow());
        assert!(FillKind::SHADOW_INSET.is_shadow());
        assert!(!FillKind::SOLID.is_shadow());
        assert!(!FillKind::linear(Spread::Pad).is_shadow());
    }

    /// Pin: `Quad` is exactly 76 bytes — pos(8) + size(8) + fill(16) +
    /// radius(8, packed 4xf16) + stroke_color(16) + stroke_width(4) +
    /// fill_kind(4) + fill_lut_row(4) + fill_axis(8, packed 4xf16). The
    /// `vertex_attr_array` in the backend's `QuadPipeline::new` assumes
    /// this exact layout via Rust's `repr(C)` field-order rules. A
    /// reorder or an added field that shifts an attribute's offset would
    /// break the shader binding silently — this test catches it.
    #[test]
    fn quad_struct_is_76_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<Quad>(), 76);
    }

    /// Pin every field offset against the `vertex_attr_array!` in
    /// `quad_pipeline.rs` (attribute locations 0..=8). A reorder of
    /// same-sized fields wouldn't change the struct size but would
    /// silently mis-bind the shader; size alone can't catch it.
    #[test]
    fn quad_field_offsets_match_vertex_attr_array() {
        assert_eq!(offset_of!(Quad, rect), 0, "loc 0 (pos) + loc 1 (size)");
        assert_eq!(offset_of!(Quad, fill), 16, "loc 2 (fill)");
        assert_eq!(offset_of!(Quad, radius), 32, "loc 3 (radius, packed)");
        assert_eq!(offset_of!(Quad, stroke_color), 40, "loc 4 (stroke.color)");
        assert_eq!(offset_of!(Quad, stroke_width), 56, "loc 5 (stroke.width)");
        assert_eq!(offset_of!(Quad, fill_kind), 60, "loc 6 (fill_kind)");
        assert_eq!(offset_of!(Quad, fill_lut_row), 64, "loc 7 (fill_lut_row)");
        assert_eq!(offset_of!(Quad, fill_axis), 68, "loc 8 (fill_axis)");
    }
}
