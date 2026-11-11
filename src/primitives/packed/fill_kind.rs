//! Packed fill-brush marker: the `u32` a `Brush` lowers into for the shader. It
//! lives at the primitives layer so the shape store, record store and renderer all
//! depend *down* on one definition.

use crate::primitives::paint::brush::gradient::Spread;
use bytemuck::{Pod, Zeroable};

/// Packed fill-brush metadata for `QuadInstance.fill_kind` and the matching paint-payload
/// fields: **bits 0..8** the family tag (one of the `TAG_*`, via
/// [`Self::tag`]), **bits 8..16** the `Spread` discriminant (gradient tags only).
/// A brush alone: the shape's form and a shadow are the quad row's key (`QuadKey`). Refused: a
/// paint word that carries geometry and composer flags, which the backend read back at upload.
/// `repr(transparent)` over `u32` so the wire layout is a plain vertex attribute.
///
/// Every number here is substituted into the quad shader (`quad_pipeline/*.wgsl`) rather than
/// mirrored by hand; `every_pinned_shader_constant_is_read` fails if the shader
/// stops comparing against one.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct FillKind(pub(crate) u32);

impl FillKind {
    pub(crate) const TAG_MASK: u32 = 0xFF;
    pub(crate) const SPREAD_SHIFT: u32 = 8;
    pub(crate) const SPREAD_MASK: u32 = 0xFF;

    /// The family tags, each substituted into the shader as a `PALANTIR_FILL_*`; the
    /// only place any is written.
    pub(crate) const TAG_SOLID: u32 = 0;
    pub(crate) const TAG_LINEAR: u32 = 1;
    pub(crate) const TAG_RADIAL: u32 = 2;
    pub(crate) const TAG_CONIC: u32 = 3;
    pub(crate) const TAG_RAMP: u32 = 4;

    #[inline]
    pub(crate) const fn tag(self) -> u32 {
        self.0 & Self::TAG_MASK
    }

    /// Solid-fill marker; `QuadInstance.fill` carries the color.
    pub(crate) const SOLID: Self = Self(Self::TAG_SOLID);

    /// Linear-gradient marker with the spread in bits 8..16; the atlas row and axis
    /// ride in `QuadInstance.fill_lut_row` / `fill_axis`.
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

    /// Curve-ramp marker, read by the curve pipeline only; the atlas row rides in
    /// `fill_lut_row`, so there is no axis or spread.
    pub(crate) const RAMP: Self = Self(Self::TAG_RAMP);
}

// The fields tile the word without overlapping and every value fits its field.
const _: () = {
    assert!(FillKind::TAG_MASK < 1 << FillKind::SPREAD_SHIFT);
    assert!(FillKind::TAG_RAMP <= FillKind::TAG_MASK);
    assert!(Spread::Reflect as u32 <= FillKind::SPREAD_MASK);
};
