//! A shadow lowered to its GPU-wire form.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::shadow::Shadow;
use glam::Vec2;
use std::hash;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LoweredShadow {
    pub(crate) color: RgbaF16,
    /// `(offset.x, offset.y, blur, spread)` in the shared 4-lane [`F16x4`] core.
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
    #[inline]
    pub(crate) const fn from_lanes([x, y, blur, spread]: [f32; 4]) -> Self {
        Self {
            offset: Vec2::new(x, y),
            blur,
            spread,
        }
    }

    /// Standard deviations of blur a shadow is followed: past it the Gaussian weight (`Φ(−4) ≈ 3e-5`) is a tenth of
    /// an 8-bit step even where sRGB is steepest, whereas `Φ(−3)` would end a glow in a ledge. The quad shader takes
    /// it as a substituted constant.
    pub(crate) const REACH_SIGMAS: f32 = 4.0;

    /// How far a drop shadow reaches past its moved source: [`Self::REACH_SIGMAS`] blur deviations plus a positive
    /// spread; the AA ramp is not included, since the quad shader grows every quad by its own.
    #[inline]
    pub(crate) const fn halo(self) -> f32 {
        Self::REACH_SIGMAS * self.blur.max(0.0) + self.spread.max(0.0)
    }
}

impl LoweredShadow {
    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        // Geometry is screened for NaN, not magnitude: a zero-sigma zero-offset shadow still paints, so only the tint
        // decides visibility. Mirrors `Shadow::is_noop`; chrome reaches `emit_shadow` through this form with no NaN gate.
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

    /// Owner-local paint bbox: a drop shadow is the offset source inflated by its [halo](ShadowGeom::halo), an inset
    /// one stays inside the source. `local_rect = None` means the source covers the owner.
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

/// Fed as words straight from registers, not as struct bytes.
impl hash::Hash for LoweredShadow {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.color.as_u64());
        state.write_u64(self.geom_f16.as_u64());
        state.write_u16(self.inset_flag);
    }
}

impl NanCheck for LoweredShadow {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.geom_f16.has_nan()
    }
}
