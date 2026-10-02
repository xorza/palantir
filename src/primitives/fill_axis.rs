//! The four-lane geometry word a quad's fill reads.

use crate::primitives::half_simd::F16x4;

/// GPU-wire form of a gradient's axis: four f16 lanes (`[u16; 4]`,
/// 8 B). Variant-dependent layout — `[dir_x, dir_y, t0, t1]` for
/// linear, `[cx, cy, rx, ry]` for radial, `[cx, cy, start_angle, _]`
/// for conic, `[0, 0, σ, spread]` for drop shadows, and
/// `[offset.x, offset.y, σ, spread]` for inset shadows. Mirrors
/// `Corners`'s u64 lane scheme — the WGSL vertex attribute is
/// `vec2<u32>` and the shader unpacks via two `unpack2x16float`
/// calls into the same `vec4<f32>` the fragment shader sees.
///
/// f16 precision (~3 decimal digits) is plenty for unit direction
/// vectors and the 0..1 parametric range. A pixel-valued lane (a shadow's
/// σ, spread or offset) rounds by up to half an f16 step: ¼ px below
/// 1024, ½ px below 2048, 1 px below 4096.
///
/// A triangle quad reuses the word differently: its first two lanes are
/// the third corner as unorm16 shares of the quad, and the third its
/// corner radius in f16 — see `quad.wgsl`'s triangle notes.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FillAxis(F16x4);

impl From<F16x4> for FillAxis {
    /// Adopt an already-packed word. The inset-shadow axis is exactly
    /// `LoweredShadow::geom_f16`, so it travels packed rather than
    /// through an f16 → f32 → f16 round trip of identical bytes.
    #[inline]
    fn from(lanes: F16x4) -> Self {
        Self(lanes)
    }
}

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind == SOLID`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    /// Build from four runtime f32 lanes. Single SIMD instruction on
    /// F16C/fp16 targets.
    #[inline]
    pub(crate) fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self(F16x4::from_lanes([a, b, c, d]))
    }

    /// All four lanes unpacked at once — matches `Corners::as_array`.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Scale every lane — the composer's walk-transform scale
    /// multiply, run per quad.
    ///
    /// Delegates to [`F16x4::scaled`] rather than composing
    /// `from_lanes(lanes().map(..))`: that spelling bounces through two
    /// `[f32; 4]` arrays and measures 1.3x slower, which is the whole
    /// reason the fused form exists. `Corners::scaled_by` delegates the
    /// same way.
    #[inline]
    pub(crate) fn scaled(self, s: f32) -> Self {
        Self(self.0.scaled(s))
    }
}
