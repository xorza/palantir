//! The four-lane geometry word a quad's fill reads.

use crate::primitives::packed::half_simd::F16x4;

/// GPU-wire form of a gradient's axis: four f16 lanes (`[u16; 4]`, 8 B), laid out per variant: `[dir_x, dir_y, t0, t1]`
/// linear, `[cx, cy, rx, ry]` radial, `[cx, cy, start_angle, _]` conic.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FillAxis(F16x4);

impl FillAxis {
    /// All-zero axis for solid quads (the shader ignores it); zeroed so Pod-byte cache keys are deterministic.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    #[inline]
    pub(crate) fn from_lanes(lanes: [f32; 4]) -> Self {
        Self(F16x4::from_lanes(lanes))
    }

    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }
}
