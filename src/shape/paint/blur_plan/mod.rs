//! How a backdrop blur of one σ runs: the levels it halves through, the residual Gaussian it ends
//! with, and how far past a pane it reads.
//!
//! # Decisions
//!
//! - **Exact variance and mean from a pyramid and a residual Gaussian.** The capture is the
//!   pixels behind the pane at full resolution, level 0. A tent `[1 3 3 1]` halves it while σ in
//!   the level's texels exceeds 4 (Skia's threshold), a separable Gaussian runs at the last
//!   level, and the composite reads that level through a cubic B-spline (Sigg and Hadwiger), or
//!   texel for texel at level 0. Each stage keeps an impulse's mean and adds a known variance at
//!   every phase (Marsden's identity for the B-spline), so the residual's taps are solved to the
//!   exact remainder, cut at `ShadowGeometry::REACH_SIGMAS`; the whole stays within 0.31 % of a
//!   Gaussian's peak. Refused: a half-resolution capture, whose box moves an impulse's mean with
//!   its phase, so a blurred edge jumps as content scrolls a pixel (15.7 % of the peak at σ 2);
//!   dual Kawase, whose strength steps by iteration count; a full-resolution Gaussian at every σ;
//!   bilinear upscaling, which shows the lattice.
//! - **Every pass reads by whole texel.** Taps are `textureLoad`s with the weights in the shader.
//!   Refused: linear-sampled tap pairs, whose split weight rounds to the hardware's sub-texel
//!   precision (Vulkan allows 1/16).
//! - **The reach is the plan's exact support.** [`BlurPlan::reach`] sums every stage's support in
//!   physical px, so a capture of any larger region gives a pane's pixels bit for bit. Refused:
//!   3σ of the whole kernel.
//! - **The lattice is the surface.** The capture and the levels are anchored at the surface's
//!   origin, so a pane sliding over a still page blurs the same texels the same way every frame.
//!   Mirror applies only at the surface's edge, where nothing is painted.

use crate::primitives::geometry::urect::URect;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::UVec2;

/// A backdrop blur's stages, each of whose variances is fixed, and the residual Gaussian that takes
/// what they leave of σ², so the whole blur has variance σ² at every phase of its lattice.
///
/// The capture holds the pixels behind a pane at full resolution, level 0. A `[1 3 3 1]` tent
/// halves it while σ in the current level's texels exceeds [`Self::MAX_LEVEL_SIGMA`], the residual
/// runs at the last level, and the composite reads that level through a cubic B-spline, or texel
/// for texel at level 0. Every stage keeps an impulse's mean at every phase, so a pane's blur does
/// not shift as content scrolls under it by a pixel. Lengths are physical px, and a level's texel
/// is `2^level` of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlurPlan {
    levels: u8,
    radius: u8,
    /// The residual's weights at offsets `0..=radius`, normalized over `-radius..=radius`.
    taps: [f32; Self::MAX_TAPS],
    reach: u32,
}

impl BlurPlan {
    /// Skia's threshold: past it, a level halves again.
    pub(crate) const MAX_LEVEL_SIGMA: f32 = 4.0;
    /// The widest residual: σ of `MAX_LEVEL_SIGMA` texels, cut at `REACH_SIGMAS` of them.
    pub(crate) const MAX_RADIUS: usize = 16;
    pub(crate) const MAX_TAPS: usize = Self::MAX_RADIUS + 1;
    /// The most halvings a plan takes, which bounds σ.
    pub(crate) const MAX_LEVELS: u8 = 12;
    /// The widest σ a plan blurs by, in physical px: `MAX_LEVEL_SIGMA` texels of the last level.
    pub(crate) const MAX_SIGMA: f32 = Self::MAX_LEVEL_SIGMA * (1u32 << Self::MAX_LEVELS) as f32;

    /// Variance of the `[1 3 3 1] / 8` tent at offsets ±½ and ±1½, in its source level's texel².
    /// Each of its two phases has this variance and keeps the mean, so it holds at every phase.
    const TENT_VARIANCE: f64 = 0.75;
    /// Variance of the cubic B-spline read, in its level's texel²; by Marsden's identity it is the
    /// same at every phase.
    const BSPLINE_VARIANCE: f64 = 1.0 / 3.0;

    /// The plan of a Gaussian blur of `sigma` physical px, at most [`Self::MAX_SIGMA`]: a wider
    /// σ, or the infinity a logical σ scales to past an f16 lane, blurs as the widest.
    pub(crate) fn new(sigma: f32) -> Self {
        debug_assert!(sigma >= 0.0, "a blur σ is not negative or NaN: {sigma}");
        let sigma = sigma.min(Self::MAX_SIGMA);
        let levels = Self::levels_for(sigma);
        debug_assert!(
            levels <= Self::MAX_LEVELS,
            "σ {sigma} px halves past {} levels",
            Self::MAX_LEVELS
        );
        let texel = f64::from(1u32 << levels);
        let residual =
            ((f64::from(sigma).powi(2) - Self::fixed_variance(levels)) / (texel * texel)).max(0.0);
        let residual_taps = ResidualTaps::solve(residual);
        let reach = Self::reach_of(levels, residual_taps.radius);
        Self {
            levels,
            radius: residual_taps.radius as u8,
            taps: residual_taps.taps,
            reach,
        }
    }

    const fn levels_for(sigma: f32) -> u8 {
        let mut levels = 0;
        let mut level_sigma = sigma;
        while level_sigma > Self::MAX_LEVEL_SIGMA {
            levels += 1;
            level_sigma *= 0.5;
        }
        levels
    }

    /// What the stages around the residual add, in px²: the tents out of levels `0..levels`, and
    /// the B-spline read of the last level.
    fn fixed_variance(levels: u8) -> f64 {
        if levels == 0 {
            return 0.0;
        }
        let tents: f64 = (0..levels)
            .map(|k| Self::TENT_VARIANCE * 4f64.powi(i32::from(k)))
            .sum();
        tents + Self::BSPLINE_VARIANCE * 4f64.powi(i32::from(levels))
    }

    /// How far, in physical px, a pixel's result reads past it: the B-spline's two texels, the
    /// residual's radius, and each tent's one and a half source texels, `1.5 · (2^levels − 1)` px
    /// in all, rounded up.
    const fn reach_of(levels: u8, radius: usize) -> u32 {
        if levels == 0 {
            return radius as u32;
        }
        let texel = 1u32 << levels;
        (2 + radius as u32) * texel + (3 * (texel - 1)).div_ceil(2)
    }

    /// Halvings from the surface to the level the residual runs at; 0 is full resolution.
    pub(crate) const fn levels(self) -> u8 {
        self.levels
    }

    /// The physical px of one texel of the last level.
    pub(crate) const fn lattice(self) -> u32 {
        1 << self.levels
    }

    /// The residual's weights at offsets `0..=radius`.
    pub(crate) fn taps(&self) -> &[f32] {
        &self.taps[..=usize::from(self.radius)]
    }

    pub(crate) const fn radius(self) -> u8 {
        self.radius
    }

    /// How far past a pane, in physical px, its blur reads: the plan's exact support, so a capture
    /// of any larger region gives the pane's pixels the same values.
    pub(crate) const fn reach(self) -> u32 {
        self.reach
    }

    /// The texels around a result the composite's B-spline reads: two at a blurred level, none at
    /// full resolution, which it reads texel for texel.
    pub(crate) const fn apron(self) -> u32 {
        if self.levels == 0 { 0 } else { 2 }
    }

    /// The extent of the last level over a surface of `surface` physical px.
    pub(crate) const fn level_size(self, surface: UVec2) -> UVec2 {
        let lattice = self.lattice();
        UVec2::new(surface.x.div_ceil(lattice), surface.y.div_ceil(lattice))
    }

    /// `footprint` in texels of the last level: snapped out to the lattice and kept inside a
    /// surface of `surface` physical px. `None` when it misses the surface.
    pub(crate) fn region(self, footprint: URect, surface: UVec2) -> Option<URect> {
        let lattice = self.lattice();
        let min = footprint.min / lattice;
        let max = (footprint.max() + UVec2::splat(lattice - 1)) / lattice;
        URect::from_min_max(min, max)
            .intersect(URect::from_min_max(UVec2::ZERO, self.level_size(surface)))
    }
}

/// The residual Gaussian's taps, solved so their discrete variance is the residual's.
#[derive(Debug)]
struct ResidualTaps {
    radius: usize,
    taps: [f32; BlurPlan::MAX_TAPS],
}

impl ResidualTaps {
    /// Newton steps on the kernel's σ: from the nominal σ the variance converges in about five, and
    /// a step that leaves the bracket bisects it instead.
    const MAX_STEPS: u32 = 64;

    fn solve(variance: f64) -> Self {
        let mut taps = [0.0; BlurPlan::MAX_TAPS];
        if variance == 0.0 {
            taps[0] = 1.0;
            return Self { radius: 0, taps };
        }
        let reach = f64::from(ShadowGeometry::REACH_SIGMAS) * variance.sqrt();
        #[expect(
            clippy::cast_sign_loss,
            reason = "a multiple of a square root is not negative"
        )]
        let radius = (reach.ceil() as usize).clamp(1, BlurPlan::MAX_RADIUS);
        debug_assert!(
            variance < kernel_variance(radius, f64::INFINITY).value,
            "a residual of {variance} texel² exceeds what {radius} taps a side can hold"
        );
        let (mut lo, mut hi) = (0.0, radius as f64);
        while kernel_variance(radius, hi).value < variance {
            hi *= 2.0;
        }
        let mut sigma = variance.sqrt().clamp(lo, hi);
        for _ in 0..Self::MAX_STEPS {
            let at = kernel_variance(radius, sigma);
            let miss = at.value - variance;
            if miss.abs() <= f64::EPSILON * variance {
                break;
            }
            if miss < 0.0 {
                lo = sigma;
            } else {
                hi = sigma;
            }
            let newton = sigma - miss / at.slope;
            sigma = if newton > lo && newton < hi {
                newton
            } else {
                f64::midpoint(lo, hi)
            };
            if hi - lo <= f64::EPSILON * hi {
                break;
            }
        }
        let weights = gaussian(radius, sigma);
        let total = weights[0] + 2.0 * weights[1..=radius].iter().sum::<f64>();
        for (tap, w) in taps.iter_mut().zip(&weights[..=radius]) {
            *tap = (w / total) as f32;
        }
        Self { radius, taps }
    }
}

/// Unnormalized Gaussian weights of `sigma` at offsets `0..=radius`; an infinite σ is flat.
fn gaussian(radius: usize, sigma: f64) -> [f64; BlurPlan::MAX_TAPS] {
    let mut weights = [0.0; BlurPlan::MAX_TAPS];
    for (k, w) in weights[..=radius].iter_mut().enumerate() {
        let d = k as f64 / sigma;
        *w = (-0.5 * d * d).exp();
    }
    weights
}

/// A kernel's variance and its slope against σ.
#[derive(Clone, Copy, Debug)]
struct KernelVariance {
    value: f64,
    slope: f64,
}

/// The variance of the normalized kernel of `gaussian(radius, sigma)` over `-radius..=radius`,
/// and its derivative in σ: each weight `g = exp(−k²/2σ²)` moves by `g·k²/σ³`.
fn kernel_variance(radius: usize, sigma: f64) -> KernelVariance {
    let weights = gaussian(radius, sigma);
    let (mut total, mut moment) = (weights[0], 0.0);
    let (mut d_total, mut d_moment) = (0.0, 0.0);
    let cube = sigma.powi(3);
    for (k, w) in weights[1..=radius].iter().enumerate() {
        let k2 = ((k + 1) * (k + 1)) as f64;
        let dw = w * k2 / cube;
        total += 2.0 * w;
        moment += 2.0 * w * k2;
        d_total += 2.0 * dw;
        d_moment += 2.0 * dw * k2;
    }
    KernelVariance {
        value: moment / total,
        slope: (d_moment * total - moment * d_total) / (total * total),
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::shape::paint::blur_plan::BlurPlan;

    /// The blur's arithmetic in `f64` on one axis, stage by stage as the GPU runs it, with the
    /// mirror at a level's edge: the reference the GPU passes and the variance claims are checked
    /// against.
    impl BlurPlan {
        /// `pixels`, the capture, taken through each tent to the last level.
        pub(crate) fn reference_level(self, pixels: &[f64]) -> Vec<f64> {
            let mut level = pixels.to_vec();
            for _ in 0..self.levels {
                level = reference_tent(&level);
            }
            level
        }

        /// The residual Gaussian over `level`, mirrored at its edges.
        pub(crate) fn reference_residual(self, level: &[f64]) -> Vec<f64> {
            let radius = i64::from(self.radius);
            (0..level.len() as i64)
                .map(|i| {
                    (-radius..=radius)
                        .map(|k| {
                            f64::from(self.taps[k.unsigned_abs() as usize])
                                * level[mirror(i + k, level.len())]
                        })
                        .sum()
                })
                .collect()
        }

        /// What the composite reads at the center of pixel `n` from the blurred last `level`: the
        /// cubic B-spline over its texels, or the texel itself at full resolution.
        pub(crate) fn reference_read(self, level: &[f64], n: usize) -> f64 {
            if self.levels == 0 {
                return level[n];
            }
            let texel = f64::from(self.lattice());
            let x = (n as f64 + 0.5) / texel - 0.5;
            let base = x.floor() as i64;
            (base - 1..=base + 2)
                .map(|j| cubic_bspline(x - j as f64) * level[mirror(j, level.len())])
                .sum()
        }
    }

    /// One `[1 3 3 1] / 8` tent halving, mirrored at the edge.
    pub(crate) fn reference_tent(level: &[f64]) -> Vec<f64> {
        const WEIGHTS: [f64; 4] = [0.125, 0.375, 0.375, 0.125];
        (0..level.len().div_ceil(2) as i64)
            .map(|i| {
                WEIGHTS
                    .iter()
                    .zip(2 * i - 1..)
                    .map(|(w, j)| w * level[mirror(j, level.len())])
                    .sum()
            })
            .collect()
    }

    /// The texel `i` reads on a level of `n` texels: mirrored past an edge, the edge texel
    /// repeated, as `MirrorRepeat` addresses.
    pub(crate) fn mirror(i: i64, n: usize) -> usize {
        let n = n as i64;
        let m = i.rem_euclid(2 * n);
        usize::try_from(if m < n { m } else { 2 * n - 1 - m }).unwrap()
    }

    /// The centered cubic B-spline.
    pub(crate) fn cubic_bspline(t: f64) -> f64 {
        let t = t.abs();
        if t < 1.0 {
            (4.0 - 6.0 * t * t + 3.0 * t * t * t) / 6.0
        } else if t < 2.0 {
            (2.0 - t).powi(3) / 6.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests;
