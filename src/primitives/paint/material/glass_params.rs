//! [`GlassParams`]: what the stock glass reads.

use crate::primitives::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::primitives::paint::material::lens::Lens;
use std::mem::offset_of;

/// The parameters of [`EffectName::GLASS`](crate::EffectName::GLASS): its saturation and grain, as
/// [`FrostedParams`](crate::FrostedParams)'s, then the lens it bends what it reads through. The
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
pub struct GlassParams {
    /// The color's saturation as CSS `saturate()` scales it; 1 leaves it.
    pub saturation: f32,
    /// The amplitude of a grain fixed to the material; 0 is none.
    pub noise: f32,
    /// The glass.
    pub lens: Lens,
}

impl GlassParams {
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
impl EffectParams for GlassParams {
    const FIELDS: &'static [EffectParam] = &[
        f32_field("saturation", offset_of!(Self, saturation)),
        f32_field("noise", offset_of!(Self, noise)),
        f32_field("bezel", lens_field(offset_of!(Lens, bezel))),
        f32_field("thickness", lens_field(offset_of!(Lens, thickness))),
        f32_field(
            "refractive_index",
            lens_field(offset_of!(Lens, refractive_index)),
        ),
        f32_field("dispersion", lens_field(offset_of!(Lens, dispersion))),
        f32_field("highlight", lens_field(offset_of!(Lens, highlight))),
    ];
}

/// The offset in [`GlassParams`] of the lens field at `offset` in [`Lens`].
const fn lens_field(offset: usize) -> usize {
    offset_of!(GlassParams, lens) + offset
}

const fn f32_field(name: &'static str, offset: usize) -> EffectParam {
    EffectParam {
        name,
        offset,
        ty: ParamType::F32,
    }
}
