//! Palantir's stock effects: the two press ripples and the three materials, which every registry
//! holds from its start under names in palantir's reserved prefix, so a theme names them as it
//! names an app's.

use crate::effect::effect_source::{EffectSource, ReducedPaint};
use crate::effect::press_input::PressInput;
use crate::primitives::effect::effect_kind::{BackdropEffect, EffectKind};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::paint::material::lens_optics::{
    LINE_C, LINE_D, LINE_F, SPECTRUM_RED, SPECTRUM_SAMPLES, SPECTRUM_VIOLET,
};
use crate::primitives::paint::material::material_input::MaterialInput;

/// The prefix of every name palantir loads itself, which
/// [`Ui::load_effect`](crate::Ui::load_effect) refuses, so an app's effect never replaces a stock
/// one a theme names.
pub(crate) const RESERVED_PREFIX: &str = "palantir.";

impl EffectName<PressInput> {
    /// Material's flat ripple, as Compose's `RippleAnimation` draws it: a disc of the press
    /// color that grows from the press to the widget's center. Material's times are 0.225 s to
    /// enter and 0.15 s to exit.
    pub const RIPPLE: Self = reserved("palantir.ripple");

    /// Android's sparkle ripple, ported from AOSP's `RippleShader`: a soft disc of the press
    /// color under a ring of sparkles of its highlight. Android's times are 0.45 s to enter and
    /// 0.375 s to exit. Under the power saver it paints [`Self::RIPPLE`], whose disc costs a
    /// fraction of the sparkles' noise.
    pub const SPARKLE: Self = reserved("palantir.sparkle");
}

impl EffectName<MaterialInput, BackdropEffect> {
    /// What the material reads, blurred by its capture's σ, as it is, as CSS
    /// `backdrop-filter: blur()` shows it. It reads none of its input.
    pub const BLUR: Self = reserved("palantir.blur");

    /// Frosted glass: what the material reads, its color saturated as CSS `saturate()` scales it
    /// and grained, so a fill over it reads as a material, as Apple's materials deepen theirs. It
    /// reads the input's saturation and grain.
    pub const FROSTED: Self = reserved("palantir.frosted");

    /// Glass: what the material reads, bent through the input's lens across a rounded bezel at
    /// its edge, every wavelength at its own index where the lens disperses, with the lamp's glint,
    /// then saturated and grained as [`Self::FROSTED`] is.
    pub const GLASS: Self = reserved("palantir.glass");
}

const fn reserved<U: EffectParams, K: EffectKind>(name: &str) -> EffectName<U, K> {
    match EffectName::named(name) {
        Some(name) => name,
        None => panic!("a stock name fits an inline name"),
    }
}

/// Each stock effect, with its source, in load order: an alternative before what names it.
pub(crate) fn sources() -> StockSources {
    StockSources {
        press: [
            (
                EffectName::RIPPLE,
                EffectSource::new(concat!(
                    include_str!("press.wgsl"),
                    include_str!("ripple.wgsl")
                )),
            ),
            (
                EffectName::SPARKLE,
                EffectSource::new(concat!(
                    include_str!("press.wgsl"),
                    include_str!("sparkle.wgsl")
                ))
                .power_saver(ReducedPaint::Effect(EffectName::RIPPLE)),
            ),
        ],
        materials: [
            (
                EffectName::BLUR,
                EffectSource::new(concat!(
                    include_str!("material.wgsl"),
                    include_str!("blur.wgsl")
                )),
            ),
            (
                EffectName::FROSTED,
                EffectSource::new(concat!(
                    include_str!("material.wgsl"),
                    include_str!("frosted.wgsl")
                )),
            ),
            (EffectName::GLASS, EffectSource::new(glass_wgsl())),
        ],
    }
}

/// The stock effects by their parameter types, as [`sources`] gives them.
#[derive(Debug)]
pub(crate) struct StockSources {
    pub(crate) press: [(EffectName<PressInput>, EffectSource<PressInput>); 2],
    pub(crate) materials: [(
        EffectName<MaterialInput, BackdropEffect>,
        EffectSource<MaterialInput, BackdropEffect>,
    ); 3],
}

/// The glass's WGSL, with the optics' constants the Rust types own.
pub(crate) fn glass_wgsl() -> String {
    format!(
        "{}{}",
        include_str!("material.wgsl"),
        include_str!("glass.wgsl")
    )
    .replace("/*{LINE_D}*/", &format!("{LINE_D:?}"))
    .replace("/*{LINE_F}*/", &format!("{LINE_F:?}"))
    .replace("/*{LINE_C}*/", &format!("{LINE_C:?}"))
    .replace("/*{SPECTRUM_RED}*/", &format!("{SPECTRUM_RED:?}"))
    .replace("/*{SPECTRUM_VIOLET}*/", &format!("{SPECTRUM_VIOLET:?}"))
    .replace("/*{SPECTRUM_SAMPLES}*/", &format!("{SPECTRUM_SAMPLES}u"))
}
