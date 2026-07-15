//! Lowered paint data shared by shape records and node chrome.

use crate::common::content_hash::ContentHash;
use crate::frame_arena::GradientId;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use glam::Vec2;
use half::f16;

#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum ShapeBrush {
    Solid(ColorF16),
    Gradient(GradientId),
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeStroke {
    pub(crate) color: ColorF16,
    pub(crate) width_f16: u16,
}

impl ShapeStroke {
    #[inline]
    pub(crate) fn width(self) -> f32 {
        f16::from_bits(self.width_f16).to_f32()
    }

    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        use crate::primitives::approx::noop_f16_bits;
        noop_f16_bits(self.width_f16) || self.color.is_noop()
    }
}

impl From<&Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: &Stroke) -> Self {
        Self {
            color: ColorF16::from(stroke.brush.expect_solid()),
            width_f16: f16::from_f32(stroke.width).to_bits(),
        }
    }
}

impl From<Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: Stroke) -> Self {
        Self::from(&stroke)
    }
}

impl From<ShapeStroke> for Stroke {
    #[inline]
    fn from(stroke: ShapeStroke) -> Self {
        Stroke::solid(Color::from(stroke.color), stroke.width())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) stroke: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    pub(crate) hash: ContentHash,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredShadow {
    pub(crate) color: ColorF16,
    pub(crate) geom_f16: [u16; 4],
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
    pub(crate) fn is_noop(self) -> bool {
        self.color.is_noop()
    }

    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        use crate::primitives::half_simd::f16x4_to_f32x4;
        let out = f16x4_to_f32x4(self.geom_f16);
        ShadowGeom {
            offset: Vec2::new(out[0], out[1]),
            blur: out[2],
            spread: out[3],
        }
    }

    #[inline]
    pub(crate) fn inset(self) -> bool {
        self.inset_flag != 0
    }
}

impl From<Shadow> for LoweredShadow {
    #[inline]
    fn from(shadow: Shadow) -> Self {
        use crate::primitives::half_simd::f16x4_from_f32x4;
        let geom_f16 =
            f16x4_from_f32x4([shadow.offset.x, shadow.offset.y, shadow.blur, shadow.spread]);
        Self {
            color: shadow.color.into(),
            geom_f16,
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
