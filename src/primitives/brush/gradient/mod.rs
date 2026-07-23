use crate::primitives::brush::gradient::stops::{MAX_STOPS, Stop};
use crate::primitives::half_simd::F16x4;
use tinyvec::ArrayVec;

macro_rules! gradient_builder_common {
    ($t:ty) => {
        impl $t {
            /// Add a color stop at `offset`, clamped to the 0..=1 gradient range.
            pub fn stop(mut self, offset: f32, color: impl Into<ColorU8>) -> Self {
                self.common.push(Stop::new(offset, color));
                self
            }

            pub fn with_spread(mut self, spread: Spread) -> Self {
                self.common.spread = spread;
                self
            }

            pub fn with_interp(mut self, interp: Interp) -> Self {
                self.common.interp = interp;
                self
            }
        }
    };
}

macro_rules! gradient_common {
    ($t:ty) => {
        impl $t {
            /// Override how the gradient repeats outside the 0..1
            /// parametric range. Builder-style.
            pub const fn with_spread(mut self, spread: Spread) -> Self {
                self.spread = spread;
                self
            }

            /// Override the colour space interpolation runs in.
            /// Builder-style.
            pub const fn with_interp(mut self, interp: Interp) -> Self {
                self.interp = interp;
                self
            }

            /// Paints nothing visible when every stop is transparent.
            #[inline]
            pub fn is_noop(&self) -> bool {
                self.stops.iter().all(|stop| stop.color.is_noop())
            }
        }
    };
}

pub(crate) mod conic;
pub(crate) mod linear;
pub(crate) mod radial;
pub(crate) mod stops;

/// How the gradient repeats outside the 0..1 parametric range.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
pub enum Spread {
    /// Clamp to nearest edge stop. CSS default.
    #[default]
    Pad = 0,
    /// Tile 0..1 across the surface.
    Repeat = 1,
    /// Tile mirrored.
    Reflect = 2,
}

/// Colour space the interpolation runs in. Affects the perceived
/// transition; doesn't change the stop colours themselves.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
pub enum Interp {
    /// Perceptually uniform; matches CSS Color 4 default. Avoids the
    /// muddy midpoint of complementary-colour pairs (red↔green,
    /// blue↔orange).
    #[default]
    Oklab,
    /// Linear-RGB interpolation. Cheapest; what most rendering engines
    /// do by default. Visible midpoint dip on saturated complementary
    /// pairs.
    Linear,
}

#[derive(Clone, Debug)]
struct GradientBuilderCore {
    stops: ArrayVec<[Stop; MAX_STOPS]>,
    spread: Spread,
    interp: Interp,
}

impl GradientBuilderCore {
    fn new(interp: Interp) -> Self {
        Self {
            stops: ArrayVec::new(),
            spread: Spread::default(),
            interp,
        }
    }

    fn push(&mut self, stop: Stop) {
        assert!(
            self.stops.len() < MAX_STOPS,
            "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}",
        );
        self.stops.push(stop);
    }
}

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
/// vectors and the 0..1 parametric range; sub-pixel error envelope
/// up to ~2048 px, then degrading like `Corners`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FillAxis(F16x4);

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind == SOLID`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    /// Build from four runtime f32 lanes via the batched f16 slice
    /// path. Single SIMD instruction on F16C/fp16 targets.
    #[inline]
    pub(crate) fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self(F16x4::from_lanes([a, b, c, d]))
    }

    /// All four lanes unpacked at once via the batched slice path —
    /// matches `Corners::as_array`.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Per-lane f32 setter helper for the composer's
    /// `current_transform.scale` multiply path. Re-quantizes via the
    /// scalar f16 round-trip.
    #[inline]
    pub(crate) fn scaled(self, s: f32) -> Self {
        let [a, b, c, d] = self.lanes();
        Self::from_lanes(a * s, b * s, c * s, d * s)
    }
}

#[inline]
const fn gradient_tag(spread: Spread, interp: Interp) -> u64 {
    ((spread as u64) << 8) | interp as u64
}
