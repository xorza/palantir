//! [`FrostedParams`]: what the stock frosted glass reads.

/// The parameters of [`EffectName::FROSTED`](crate::EffectName::FROSTED): what it reads saturated
/// as CSS `saturate()` scales it and grained. The WGSL declares, field for field:
///
/// ```wgsl
/// struct Params {
///     saturation: f32,
///     noise: f32,
/// }
/// ```
#[repr(C)]
#[derive(
    Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams,
)]
#[must_use]
pub struct FrostedParams {
    /// The color's saturation as CSS `saturate()` scales it: 1 leaves it, 0 is gray, above 1
    /// deepens it.
    pub saturation: f32,
    /// The amplitude of a grain fixed to the material, so frosted glass does not read as a flat
    /// color; 0 is none, and a few hundredths show it.
    pub noise: f32,
}

impl FrostedParams {
    /// Apple's frost: the color deepened by 1.8 and grained by 0.04, so a fill over it reads as a
    /// material.
    pub const DEFAULT: Self = Self {
        saturation: 1.8,
        noise: 0.04,
    };
}
