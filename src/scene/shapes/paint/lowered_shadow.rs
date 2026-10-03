//! A shadow lowered to its GPU-wire form.

use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::half_simd::F16x4;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use glam::Vec2;

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
        let out = self.geom_f16.lanes();
        ShadowGeom {
            offset: Vec2::new(out[0], out[1]),
            blur: out[2],
            spread: out[3],
        }
    }

    #[inline]
    pub(crate) const fn inset(self) -> bool {
        self.inset_flag != 0
    }

    /// Owner-local paint bbox of this shadow — a drop shadow is the
    /// offset source inflated by `3σ + max(spread, 0)`; an inset shadow
    /// stays inside the source. `local_rect = None` ⇒ source covers the
    /// full owner; `Some(r)` ⇒ source is `r` at owner-relative coords.
    ///
    /// **Sole formula source** for the shadow paint extent: the encoder
    /// (per-quad paint rect), the cascade (per-node ink union), and
    /// [`QuadShape::bbox_local`](crate::scene::shapes::paint::quad_shape::QuadShape::bbox_local) all call this, so the three views
    /// cannot drift.
    pub(crate) fn paint_rect_local(self, local_rect: Option<Rect>, owner_size: Size) -> Rect {
        let source = local_rect.unwrap_or(Rect {
            min: Vec2::ZERO,
            size: owner_size,
        });
        if self.inset() {
            return source;
        }
        let ShadowGeom {
            offset,
            blur,
            spread,
        } = self.geom();
        let halo = 3.0 * blur.max(0.0) + spread.max(0.0);
        Rect {
            min: source.min + offset,
            size: source.size,
        }
        .inflated(halo)
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
            inset_flag: shadow.inset as u16,
        }
    }
}

impl std::hash::Hash for LoweredShadow {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl NanCheck for LoweredShadow {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.geom_f16.has_nan()
    }
}
