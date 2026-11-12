//! [`GlassInput`]: what the stock glass reads.

use crate::primitives::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::primitives::paint::material::lens::Lens;

/// The parameters of [`EffectName::GLASS`](crate::EffectName::GLASS): its saturation and grain, as
/// [`FrostedInput`](crate::FrostedInput)'s, then the lens it bends what it reads through. The
/// WGSL declares, field for field, the lens flat:
///
/// ```wgsl
/// struct Params {
///     saturation: f32,
///     noise: f32,
///     bezel: f32,
///     thickness: f32,
///     refractive_index: f32,
///     dispersion: f32,
///     highlight: f32,
/// }
/// ```
///
/// The lens's lengths reach the effect in logical px, as every length an effect reads does.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct GlassInput {
    /// The color's saturation as CSS `saturate()` scales it; 1 leaves it.
    pub saturation: f32,
    /// The amplitude of a grain fixed to the material; 0 is none.
    pub noise: f32,
    /// The glass.
    pub lens: Lens,
}

impl GlassInput {
    /// Clear glass of `lens`: no saturation and no grain.
    pub const fn new(lens: Lens) -> Self {
        Self {
            saturation: 1.0,
            noise: 0.0,
            lens,
        }
    }
}

/// By hand, as the lens is a struct of its own and the WGSL reads its fields flat.
impl EffectParams for GlassInput {
    const FIELDS: &'static [EffectParam] = &[
        f32_field("saturation", 0),
        f32_field("noise", 4),
        f32_field("bezel", 8),
        f32_field("thickness", 12),
        f32_field("refractive_index", 16),
        f32_field("dispersion", 20),
        f32_field("highlight", 24),
    ];
}

const fn f32_field(name: &'static str, offset: usize) -> EffectParam {
    EffectParam {
        name,
        offset,
        ty: ParamType::F32,
    }
}
