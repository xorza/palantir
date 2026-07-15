//! Lowered-paint GPU-wire vocabulary: the packed `u32` markers a
//! `Brush` lowers into for the shader. Kept at the primitives layer so
//! the shape store (`forest::shapes`), the record store, and the
//! renderer all depend *down* on one definition instead of `forest`
//! reaching up into `renderer`. The matching gradient *axis* lives in
//! [`crate::primitives::brush::FillAxis`]; the actual LUT atlas texture
//! is a renderer resource ([`crate::renderer::gradient_atlas`]).

use crate::primitives::brush::Spread;
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
/// without updating WGSL silently desyncs; the const asserts in
/// `renderer::quad` (next to the shader) and the slice-2 visual
/// goldens catch it.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct FillKind(pub(crate) u32);

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

    /// Rounded-triangle SDF marker. `fill: Color` is the solid fill; the
    /// three corner points (packed into the reused `corners` + `fill_axis`
    /// lanes as `(a.x,a.y,b.x,b.y)` / `(c.x,c.y,radius,_)`) and the corner
    /// radius drive `sdf_triangle - radius` in the shader. Stroke rides the
    /// usual `stroke_color` / `stroke_width` fields.
    pub(crate) const TRIANGLE: Self = Self(6);

    /// Bit 16: fragment fast path. Set by the composer on a solid,
    /// sharp, stroke-less quad whose physical rect is pixel-aligned —
    /// every rasterized fragment is then interior (SDF coverage exactly
    /// 1.0), so the shader returns the premultiplied fill directly and
    /// skips the SDF + composite path, bitwise-identically. Kept in
    /// lockstep with `FILL_FLAG_FAST` in `quad.wgsl`.
    pub(crate) const FAST_BIT: u32 = 1 << 16;

    /// Bit 17: windowed rect — the fill coverage is inverted, painting
    /// the region *outside* the rounded boundary (the corner wedges out
    /// to the quad edge) while the interior stays transparent; the
    /// stroke keeps its usual inner-edge annulus. Set at
    /// `draw_rect_window` time so it rides the payload into the `Quad`
    /// untouched. Kept in lockstep with `FILL_FLAG_WINDOW` in
    /// `quad.wgsl`. Load-bearing side effect: the composer's
    /// opaque-cover checks (clear fold, fast path, occlusion prune) all
    /// compare `fill_kind == FillKind::SOLID` *exactly*, so this bit
    /// disqualifies windowed quads from being treated as opaque covers
    /// — their interior is a hole.
    pub(crate) const WINDOW_BIT: u32 = 1 << 17;

    /// Tag this kind with the fragment fast-path bit (see [`Self::FAST_BIT`]).
    #[inline]
    pub(crate) const fn with_fast(self) -> Self {
        Self(self.0 | Self::FAST_BIT)
    }

    /// Tag this kind with the inverted-fill window bit (see
    /// [`Self::WINDOW_BIT`]).
    #[inline]
    pub(crate) const fn with_window(self) -> Self {
        Self(self.0 | Self::WINDOW_BIT)
    }

    /// True iff this `FillKind` marks a shadow draw. Shadow blur
    /// extends visually past the stored rect, so shadows are never
    /// safe to drop in the occlusion-prune sweep — checked at
    /// `Composer::flush` time before marking a quad for removal.
    #[inline]
    pub(crate) const fn is_shadow(self) -> bool {
        let kind = self.0 & 0xFF;
        kind == Self::SHADOW_DROP.0 || kind == Self::SHADOW_INSET.0
    }
}

/// Index into the gradient LUT atlas texture. `LutRow(0)` is the
/// magenta debug fallback (so a stray default value paints obviously
/// wrong); real registrations occupy `1..ATLAS_ROWS`. Newtype keeps
/// the atlas-row identifier from being silently swapped with another
/// `u32` field on `Quad`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct LutRow(pub(crate) u32);

impl LutRow {
    /// Sentinel for solid (non-gradient) quads. The shader only samples
    /// the LUT when `fill_kind` is a gradient, so the value is unused
    /// in that path; a stray `FALLBACK` reaching the sampler paints
    /// magenta.
    pub(crate) const FALLBACK: LutRow = LutRow(0);
}
