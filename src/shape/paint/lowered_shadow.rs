//! A shadow lowered to its GPU-wire form.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::shadow::Shadow;
use glam::Vec2;
use std::hash;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredShadow {
    pub(crate) color: RgbaF16,
    /// `(offset.x, offset.y, blur, spread)`. Wraps [`F16x4`] rather
    /// than a bare `[u16; 4]` for the reason that type exists: it is
    /// the shared 4-lane storage core, and a field that stores the
    /// lanes raw is a field that has to re-derive every lane idiom —
    /// pack, unpack, and the NaN screen — by hand.
    pub(crate) geom_f16: F16x4,
    pub(crate) inset_flag: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowGeom {
    pub(crate) offset: Vec2,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
}

impl ShadowGeom {
    /// The `(offset.x, offset.y, blur, spread)` lanes a shadow travels in,
    /// from the lowered shadow to the GPU instance.
    #[inline]
    pub(crate) const fn from_lanes([x, y, blur, spread]: [f32; 4]) -> Self {
        Self {
            offset: Vec2::new(x, y),
            blur,
            spread,
        }
    }

    /// How many standard deviations of blur [`Self::halo`] reaches. The
    /// quad shader takes it as a substituted constant, to find the source
    /// inside a drop shadow's quad.
    pub(crate) const HALO_SIGMAS: f32 = 3.0;

    /// How far a drop shadow reaches past its moved source:
    /// [`Self::HALO_SIGMAS`] standard deviations of blur, where the
    /// Gaussian's tail drops below one 8-bit step, plus a positive spread.
    #[inline]
    pub(crate) const fn halo(self) -> f32 {
        Self::HALO_SIGMAS * self.blur.max(0.0) + self.spread.max(0.0)
    }
}

impl LoweredShadow {
    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        // Geometry screened for NaN, not magnitude — a zero-sigma
        // zero-offset shadow still paints a hard-edged rect, so only
        // the tint's *size* decides visibility. Mirrors
        // `Shadow::is_noop` one tier up; needed separately because
        // chrome reaches `emit_shadow` through this lowered form, and
        // chrome has no record-level NaN gate behind it.
        self.color.is_noop() || self.geom_f16.has_nan()
    }

    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        ShadowGeom::from_lanes(self.geom_f16.lanes())
    }

    #[inline]
    pub(crate) const fn inset(self) -> bool {
        self.inset_flag != 0
    }

    /// Owner-local paint bbox of this shadow — a drop shadow is the
    /// offset source inflated by its [halo](ShadowGeom::halo); an inset
    /// shadow stays inside the source. `local_rect = None` ⇒ source covers
    /// the full owner; `Some(r)` ⇒ source is `r` at owner-relative coords.
    ///
    /// What the cascade (per-node ink union) and
    /// [`QuadShape::bbox_local`](crate::shape::paint::quad_shape::QuadShape::bbox_local)
    /// read, so the two views cannot drift. The composer grows the same
    /// way from the snapped source, through the same `halo`.
    pub(crate) fn paint_rect_local(self, local_rect: Option<Rect>, owner_size: Size) -> Rect {
        let source = local_rect.unwrap_or(Rect {
            min: Vec2::ZERO,
            size: owner_size,
        });
        if self.inset() {
            return source;
        }
        let geom = self.geom();
        Rect {
            min: source.min + geom.offset,
            size: source.size,
        }
        .inflated(geom.halo())
    }
}

impl From<Shadow> for LoweredShadow {
    #[inline]
    fn from(shadow: Shadow) -> Self {
        Self {
            color: shadow.color.into(),
            geom_f16: F16x4::from_lanes([
                shadow.offset.x,
                shadow.offset.y,
                shadow.blur,
                shadow.spread,
            ]),
            inset_flag: u16::from(shadow.inset),
        }
    }
}

impl hash::Hash for LoweredShadow {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl NanCheck for LoweredShadow {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.geom_f16.has_nan()
    }
}
