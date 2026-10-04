//! Packed fill-brush marker: the `u32` a `Brush` lowers into for the
//! shader.
//!
//! Kept at the primitives layer so the shape store (`shape::shapes`),
//! the record store, and the renderer all depend *down* on one
//! definition instead of `forest` reaching up into `renderer`. The
//! matching gradient *axis* lives in
//! [`crate::primitives::packed::fill_axis::FillAxis`]; the atlas row the
//! gradient kinds index is [`LutRow`](crate::primitives::paint::lut_row::LutRow)
//! and the LUT texture itself is a renderer resource
//! ([`crate::renderer::gradient_atlas`]).

use crate::primitives::paint::brush::gradient::Spread;
use bytemuck::{Pod, Zeroable};

/// Packed fill-brush metadata for `Quad.fill_kind` and the matching
/// paint-payload fields:
///
/// - **bits 0..8** — the family tag, one of the eight `TAG_*` below,
///   read through [`Self::tag`];
/// - **bits 8..16** — the `Spread` discriminant, carried by the three
///   gradient tags and ignored by the rest;
/// - **bit 16** — [`Self::FAST_BIT`]; **bit 17** — [`Self::WINDOW_BIT`].
///
/// `repr(transparent)` over `u32` so the GPU wire layout is just a
/// `u32` vertex attribute — `vertex_attr_array![..., 6 => Uint32, ...]`
/// in the pipeline matches the shader's `@location(6) fill_kind: u32`
/// against this wrapper directly.
///
/// **Shader-side mapping** (`quad_pipeline/shader.wgsl`): every number here is
/// substituted into the shader rather than mirrored by hand, so a
/// reorder cannot desync the two — and
/// `every_pinned_shader_constant_is_read` fails if the shader stops
/// comparing against one it is given.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct FillKind(pub(crate) u32);

impl FillKind {
    /// The bits of the family tag: the low byte.
    pub(crate) const TAG_MASK: u32 = 0xFF;
    /// Where the `Spread` discriminant starts.
    pub(crate) const SPREAD_SHIFT: u32 = 8;
    /// The bits of the `Spread` discriminant, once shifted down by
    /// [`Self::SPREAD_SHIFT`].
    pub(crate) const SPREAD_MASK: u32 = 0xFF;

    /// The family tags. Each is substituted into the shader that branches
    /// on it as a `BRUSH_KIND_*` and compared there against the
    /// [`Self::TAG_MASK`] bits, so these are the numbers both sides agree
    /// on — and the only place any of them is written.
    pub(crate) const TAG_SOLID: u32 = 0;
    pub(crate) const TAG_LINEAR: u32 = 1;
    pub(crate) const TAG_RADIAL: u32 = 2;
    pub(crate) const TAG_CONIC: u32 = 3;
    pub(crate) const TAG_SHADOW_DROP: u32 = 4;
    pub(crate) const TAG_SHADOW_INSET: u32 = 5;
    pub(crate) const TAG_TRIANGLE: u32 = 6;
    pub(crate) const TAG_RAMP: u32 = 7;

    /// This kind's family, without the spread or the flag bits — what
    /// every predicate below and the shader's `eval_fill` branch on.
    #[inline]
    pub(crate) const fn tag(self) -> u32 {
        self.0 & Self::TAG_MASK
    }

    /// Solid-fill marker; `Quad.fill: RgbaF32` carries the colour, the
    /// LUT / axis / row fields are ignored by the shader.
    pub(crate) const SOLID: Self = Self(Self::TAG_SOLID);

    /// Linear-gradient marker with the spread mode packed into bits
    /// 8..16. The atlas row id and axis vector ride along in
    /// `Quad.fill_lut_row` / `Quad.fill_axis`.
    pub(crate) const fn linear(spread: Spread) -> Self {
        Self::gradient(Self::TAG_LINEAR, spread)
    }

    /// Radial-gradient marker. `fill_axis` carries `(cx, cy, rx, ry)`
    /// in object-space 0..1 coords; the shader projects each fragment
    /// onto the elliptical radius to derive `t`.
    pub(crate) const fn radial(spread: Spread) -> Self {
        Self::gradient(Self::TAG_RADIAL, spread)
    }

    /// Conic-gradient marker. `fill_axis` carries `(cx, cy,
    /// start_angle, _)`; the shader uses `atan2` to derive `t`.
    pub(crate) const fn conic(spread: Spread) -> Self {
        Self::gradient(Self::TAG_CONIC, spread)
    }

    /// A gradient tag with its spread in bits 8..16 — the one place the
    /// two halves are packed together.
    #[inline]
    const fn gradient(tag: u32, spread: Spread) -> Self {
        Self(tag | ((spread as u32) << Self::SPREAD_SHIFT))
    }

    /// Drop-shadow marker. `fill: RgbaF32` carries the shadow colour,
    /// `fill_axis = (0, 0, sigma, spread)`,
    /// `radius` carries the *source* shape's corner radii (the shadow
    /// wraps the source rect centered in its shifted paint bbox). The
    /// shader runs `shadow_coverage` and multiplies
    /// `fill.rgb * fill.a * cov`.
    pub(crate) const SHADOW_DROP: Self = Self(Self::TAG_SHADOW_DROP);

    /// Inset-shadow marker. `fill_axis = (offset.x, offset.y, sigma,
    /// spread)`; the shader inverts coverage and clips to inside the
    /// source rect.
    pub(crate) const SHADOW_INSET: Self = Self(Self::TAG_SHADOW_INSET);

    /// Rounded-triangle SDF marker. `fill: RgbaF32` is the solid fill; the
    /// three corner points (packed into the reused `corners` + `fill_axis`
    /// lanes as `(a.x,a.y,b.x,b.y)` / `(c.x,c.y,radius,_)`) and the corner
    /// radius drive `sdf_triangle - radius` in the shader. Stroke rides the
    /// usual `stroke_color` / `stroke_width` fields.
    pub(crate) const TRIANGLE: Self = Self(Self::TAG_TRIANGLE);

    /// Curve-ramp marker, read by the curve pipeline only. The atlas row
    /// rides in `fill_lut_row`; the curve parameter `t` samples it, so
    /// there is no axis and no spread.
    pub(crate) const RAMP: Self = Self(Self::TAG_RAMP);

    /// Bit 16: fragment fast path. Set by the composer on a solid,
    /// sharp, stroke-less quad whose physical rect is pixel-aligned —
    /// every rasterized fragment is then interior (SDF coverage exactly
    /// 1.0), so the shader returns the premultiplied fill directly and
    /// skips the SDF + composite path, bitwise-identically. The shader
    /// reads it as `FILL_FLAG_FAST`.
    pub(crate) const FAST_BIT: u32 = 1 << 16;

    /// Bit 17: windowed rect — the fill coverage is inverted, painting
    /// the region *outside* the rounded boundary (the corner wedges out
    /// to the quad edge) while the interior stays transparent; the
    /// stroke keeps its usual inner-edge annulus. Set at
    /// `draw_rect_window` time so it rides the payload into the `Quad`
    /// untouched. The shader reads it as `FILL_FLAG_WINDOW`. Load-bearing
    /// side effect: the composer's
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
        matches!(self.tag(), Self::TAG_SHADOW_DROP | Self::TAG_SHADOW_INSET)
    }
}

// The fields tile the word without overlapping, and every value fits its
// field: the shader masks each one out by these same numbers.
const _: () = {
    assert!(FillKind::TAG_MASK < 1 << FillKind::SPREAD_SHIFT);
    assert!(FillKind::TAG_RAMP <= FillKind::TAG_MASK);
    assert!(Spread::Reflect as u32 <= FillKind::SPREAD_MASK);
    let spread_bits = FillKind::SPREAD_MASK << FillKind::SPREAD_SHIFT;
    assert!(spread_bits & (FillKind::FAST_BIT | FillKind::WINDOW_BIT) == 0);
    assert!(FillKind::FAST_BIT & FillKind::WINDOW_BIT == 0);
};
