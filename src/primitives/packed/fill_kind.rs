//! Packed fill-brush marker: the `u32` a `Brush` lowers into for the shader. It
//! lives at the primitives layer so the shape store, record store and renderer all
//! depend *down* on one definition.

use crate::primitives::paint::brush::gradient::Spread;
use bytemuck::{Pod, Zeroable};

/// Packed fill-brush metadata for `Quad.fill_kind` and the matching paint-payload
/// fields: **bits 0..8** the family tag (one of the eight `TAG_*`, via
/// [`Self::tag`]), **bits 8..16** the `Spread` discriminant (gradient tags only),
/// **bit 16** [`Self::FAST_BIT`], **bit 17** [`Self::WINDOW_BIT`].
/// `repr(transparent)` over `u32` so the wire layout is a plain vertex attribute.
///
/// Every number here is substituted into `quad_pipeline/shader.wgsl` rather than
/// mirrored by hand; `every_pinned_shader_constant_is_read` fails if the shader
/// stops comparing against one.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct FillKind(pub(crate) u32);

impl FillKind {
    pub(crate) const TAG_MASK: u32 = 0xFF;
    pub(crate) const SPREAD_SHIFT: u32 = 8;
    pub(crate) const SPREAD_MASK: u32 = 0xFF;

    /// The family tags, each substituted into the shader as a `BRUSH_KIND_*`; the
    /// only place any is written.
    pub(crate) const TAG_SOLID: u32 = 0;
    pub(crate) const TAG_LINEAR: u32 = 1;
    pub(crate) const TAG_RADIAL: u32 = 2;
    pub(crate) const TAG_CONIC: u32 = 3;
    pub(crate) const TAG_SHADOW_DROP: u32 = 4;
    pub(crate) const TAG_SHADOW_INSET: u32 = 5;
    pub(crate) const TAG_TRIANGLE: u32 = 6;
    pub(crate) const TAG_RAMP: u32 = 7;

    #[inline]
    pub(crate) const fn tag(self) -> u32 {
        self.0 & Self::TAG_MASK
    }

    /// Solid-fill marker; `Quad.fill` carries the colour.
    pub(crate) const SOLID: Self = Self(Self::TAG_SOLID);

    /// Linear-gradient marker with the spread in bits 8..16; the atlas row and axis
    /// ride in `Quad.fill_lut_row` / `fill_axis`.
    pub(crate) const fn linear(spread: Spread) -> Self {
        Self::gradient(Self::TAG_LINEAR, spread)
    }

    /// Radial-gradient marker: `fill_axis` is `(cx, cy, rx, ry)` in object-space
    /// 0..1.
    pub(crate) const fn radial(spread: Spread) -> Self {
        Self::gradient(Self::TAG_RADIAL, spread)
    }

    /// Conic-gradient marker: `fill_axis` is `(cx, cy, start_angle, _)`.
    pub(crate) const fn conic(spread: Spread) -> Self {
        Self::gradient(Self::TAG_CONIC, spread)
    }

    /// A gradient tag with its spread packed in; the one place the halves are
    /// combined.
    #[inline]
    const fn gradient(tag: u32, spread: Spread) -> Self {
        Self(tag | ((spread as u32) << Self::SPREAD_SHIFT))
    }

    /// Drop-shadow marker: `fill` is the shadow colour, `fill_axis = (offset.x,
    /// offset.y, sigma, spread)`, `corners` the *source* shape's radii. The quad is
    /// the source moved by the offset and grown by the halo; the shader recovers
    /// the source from the quad's centre.
    pub(crate) const SHADOW_DROP: Self = Self(Self::TAG_SHADOW_DROP);

    /// Inset-shadow marker: same `fill_axis` layout; the shader inverts coverage
    /// and clips to the source rect.
    pub(crate) const SHADOW_INSET: Self = Self(Self::TAG_SHADOW_INSET);

    /// Rounded-triangle SDF marker: the corner points ride in the reused `corners`
    /// + `fill_axis` lanes as `(a.x,a.y,b.x,b.y)` / `(c.x,c.y,radius,_)`.
    pub(crate) const TRIANGLE: Self = Self(Self::TAG_TRIANGLE);

    /// Curve-ramp marker, read by the curve pipeline only; the atlas row rides in
    /// `fill_lut_row`, so there is no axis or spread.
    pub(crate) const RAMP: Self = Self(Self::TAG_RAMP);

    /// Bit 16: fragment fast path, set by the composer on a solid, sharp,
    /// stroke-less, pixel-aligned quad whose fragments are all interior, so the
    /// shader skips the SDF + composite path. Read as `FILL_FLAG_FAST`.
    pub(crate) const FAST_BIT: u32 = 1 << 16;

    /// Bit 17: windowed rect. Fill coverage is inverted (painting outside the
    /// rounded boundary, interior transparent); set at `draw_rect_window` time and
    /// read as `FILL_FLAG_WINDOW`. It disqualifies the quad from opaque-cover
    /// checks (clear fold, fast path, occlusion prune), which compare `fill_kind ==
    /// FillKind::SOLID` exactly, since its interior is a hole.
    pub(crate) const WINDOW_BIT: u32 = 1 << 17;

    #[inline]
    pub(crate) const fn with_fast(self) -> Self {
        Self(self.0 | Self::FAST_BIT)
    }

    #[inline]
    pub(crate) const fn with_window(self) -> Self {
        Self(self.0 | Self::WINDOW_BIT)
    }

    #[inline]
    pub(crate) const fn is_window(self) -> bool {
        self.0 & Self::WINDOW_BIT != 0
    }

    /// True iff this marks a shadow draw; blur extends past the stored rect, so
    /// shadows are never dropped by the occlusion-prune sweep.
    #[inline]
    pub(crate) const fn is_shadow(self) -> bool {
        matches!(self.tag(), Self::TAG_SHADOW_DROP | Self::TAG_SHADOW_INSET)
    }
}

// The fields tile the word without overlapping and every value fits its field.
const _: () = {
    assert!(FillKind::TAG_MASK < 1 << FillKind::SPREAD_SHIFT);
    assert!(FillKind::TAG_RAMP <= FillKind::TAG_MASK);
    assert!(Spread::Reflect as u32 <= FillKind::SPREAD_MASK);
    let spread_bits = FillKind::SPREAD_MASK << FillKind::SPREAD_SHIFT;
    assert!(spread_bits & (FillKind::FAST_BIT | FillKind::WINDOW_BIT) == 0);
    assert!(FillKind::FAST_BIT & FillKind::WINDOW_BIT == 0);
};
