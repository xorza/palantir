//! The four-lane geometry word a quad's fill reads.

use crate::primitives::packed::half_simd::F16x4;

/// GPU-wire form of a gradient's axis: four f16 lanes (`[u16; 4]`, 8 B), laid out per variant: `[dir_x, dir_y, t0, t1]`
/// linear, `[cx, cy, rx, ry]` radial, `[cx, cy, start_angle, _]` conic, `[offset.x, offset.y, σ, spread]` shadows.
/// Pixel-valued lanes round by up to half an f16 step (¼ px below 1024, ½ below 2048, 1 below 4096). A triangle
/// quad reuses the word (see `quad_pipeline/shader.wgsl`'s triangle notes).
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FillAxis(F16x4);

impl From<F16x4> for FillAxis {
    /// Adopts an already-packed word: the inset-shadow axis is `LoweredShadow::geom_f16`, skipping an f16 round trip.
    #[inline]
    fn from(lanes: F16x4) -> Self {
        Self(lanes)
    }
}

impl FillAxis {
    /// All-zero axis for solid quads (the shader ignores it); zeroed so Pod-byte cache keys are deterministic.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    #[inline]
    pub(crate) fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self(F16x4::from_lanes([a, b, c, d]))
    }

    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Scales every lane via [`F16x4::scaled`], 1.3x faster than `from_lanes(lanes().map(..))`.
    #[inline]
    pub(crate) fn scaled(self, s: f32) -> Self {
        Self(self.0.scaled(s))
    }
}
