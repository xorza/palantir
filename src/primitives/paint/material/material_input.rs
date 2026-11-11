//! [`MaterialInput`]: what a material's effect reads, as palantir fills it.

use crate::primitives::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::material::lens::Lens;

/// The parameters of a material's effect, an `EffectName<MaterialInput, BackdropEffect>` a
/// [`Material`](crate::Material) names, which palantir fills from the material for each draw, as
/// [`PressInput`](crate::PressInput) is filled for a press. An effect may read none of it: the
/// stock [`BLUR`](crate::EffectName::BLUR) reads nothing, [`FROSTED`](crate::EffectName::FROSTED)
/// the saturation and the grain, and [`GLASS`](crate::EffectName::GLASS) all of it. The WGSL
/// declares, field for field:
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
///
/// Plain data: `saturation` and `noise` are [factors](crate::widget::domain::factor), each a
/// [lane](crate::widget::domain::lane), and the lens holds its own checks.
#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(default = "MaterialInput::identity")]
#[must_use]
pub struct MaterialInput {
    /// The color's saturation as CSS `saturate()` scales it: 1 leaves it, 0 is gray, above 1
    /// deepens it.
    #[serde(
        deserialize_with = "crate::primitives::math::domain::file::lane_factor",
        skip_serializing_if = "is_one"
    )]
    pub saturation: f32,
    /// The amplitude of a grain fixed to the material, so frosted glass does not read as a flat
    /// color; 0 is none, and a few hundredths show it.
    #[serde(
        deserialize_with = "crate::primitives::math::domain::file::lane_factor",
        skip_serializing_if = "is_zero"
    )]
    pub noise: f32,
    /// The glass over it; [`Lens::NONE`] bends nothing. A file that leaves it out reads none, and
    /// one that names it reads [`Lens::WINDOW_GLASS`] where it leaves a field out.
    #[serde(default = "Lens::none", skip_serializing_if = "Lens::is_none")]
    pub lens: Lens,
}

impl MaterialInput {
    /// What changes nothing: saturation 1, no grain, no lens.
    pub const IDENTITY: Self = Self {
        saturation: 1.0,
        noise: 0.0,
        lens: Lens::NONE,
    };

    /// Frosted glass's input, which [`Material::frosted`](crate::Material::frosted) reads: the
    /// color deepened by Apple's 1.8 and grained by 0.04, so a fill over it reads as a material.
    pub const FROSTED: Self = Self {
        saturation: 1.8,
        noise: 0.04,
        lens: Lens::NONE,
    };

    /// [`Self::IDENTITY`], as serde's container default names a function.
    const fn identity() -> Self {
        Self::IDENTITY
    }

    /// Coerce each value into its kind, as a curve overshooting below a floor leaves it and where
    /// it paints: the saturation and noise factors, the lens its own ([`Lens::coerce`]). CSS
    /// clamps an interpolated value into its property's range (CSS Values 4, "Range Checking").
    pub(crate) const fn coerce(&mut self) {
        self.saturation = domain::raised_to(self.saturation, 0.0);
        self.noise = domain::raised_to(self.noise, 0.0);
        self.lens.coerce();
    }

    /// Whether `saturation` and `noise` are factor lanes and the lens valid.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        domain::are_length_lanes([self.saturation, self.noise]) & self.lens.is_valid()
    }

    /// Panics unless `saturation` and `noise` are factor lanes and the lens valid.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::factor(self.saturation));
        domain::lane(domain::factor(self.noise));
        self.lens.validate();
    }

    pub(crate) fn has_nan(&self) -> bool {
        self.saturation.is_nan() || self.noise.is_nan() || self.lens.has_nan()
    }

    /// Whether this is [`Self::IDENTITY`], which a file leaves out. Takes `&self` because serde
    /// names it in a `skip_serializing_if`.
    pub(crate) fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }
}

impl Lens {
    /// [`Self::NONE`], as serde's field default names a function.
    const fn none() -> Self {
        Self::NONE
    }

    /// Whether this is [`Self::NONE`], which a file leaves out. Takes `&self` because serde names
    /// it in a `skip_serializing_if`.
    fn is_none(&self) -> bool {
        *self == Self::NONE
    }
}

/// Whether a saturation is 1, which a file leaves out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` hands the field by reference"
)]
fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// Whether a grain is 0, which a file leaves out.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` hands the field by reference"
)]
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for MaterialInput {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Field by field, as `f32` blends.
impl Lerp for MaterialInput {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            saturation: f32::lerp(&a.saturation, &b.saturation, t),
            noise: f32::lerp(&a.noise, &b.noise, t),
            lens: Lens::lerp(&a.lens, &b.lens, t),
        }
    }
}

/// By hand, as the lens is a struct of its own and the WGSL reads its fields flat.
impl EffectParams for MaterialInput {
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
