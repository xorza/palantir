//! Palantir's stock effects: the two press ripples and the two materials, which every registry
//! holds from its start under names in palantir's reserved prefix, so a theme names them as it
//! names an app's.

use std::sync::LazyLock;

use crate::effect::effect_module::EffectModule;
use crate::effect::effect_source::{EffectSource, ReducedEffect};
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::effect::press_params::PressParams;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::glass_params::GlassParams;
use crate::primitives::paint::material::lens_optics::{
    LINE_C, LINE_D, LINE_F, SPECTRUM_RED, SPECTRUM_SAMPLES, SPECTRUM_VIOLET,
};

/// Each stock effect with its source and its module, checked once a process: the first call checks
/// them through [`EffectSource::check`], as an app's effect is, and every registry takes the
/// modules that check made, where each host and test rig parsed and validated their WGSL again, as
/// the `regex` crate's documentation compiles a pattern once into a static. Refused: checking them
/// at build time, a second load path that skips the check an app's effect takes; a cache of every
/// checked source, which would need a bound for an app's effects that no host shares.
pub(crate) fn effects() -> &'static StockEffects {
    static EFFECTS: LazyLock<StockEffects> = LazyLock::new(StockEffects::check);
    &EFFECTS
}

/// The stock effects by their parameter types, each checked, in load order: an alternative before
/// what names it.
#[derive(Debug)]
pub(crate) struct StockEffects {
    pub(crate) press: [StockEffect<PressParams, ColorEffect>; 2],
    pub(crate) frosted: StockEffect<FrostedParams, BackdropEffect>,
    pub(crate) glass: StockEffect<GlassParams, BackdropEffect>,
}

/// A stock effect's name, its source and the module checking the source made.
#[derive(Debug)]
pub(crate) struct StockEffect<U: EffectParams, K: EffectKind> {
    pub(crate) name: EffectName<U, K>,
    pub(crate) source: EffectSource<U, K>,
    pub(crate) module: EffectModule,
}

impl StockEffects {
    /// Each stock effect, checked.
    ///
    /// # Panics
    ///
    /// When a stock effect fails its contract, a logic error of palantir's.
    fn check() -> Self {
        Self {
            press: [
                StockEffect::check(
                    EffectName::RIPPLE,
                    EffectSource::new(concat!(
                        include_str!("press.wgsl"),
                        include_str!("ripple.wgsl")
                    )),
                ),
                StockEffect::check(
                    EffectName::SPARKLE,
                    EffectSource::new(concat!(
                        include_str!("press.wgsl"),
                        include_str!("sparkle.wgsl")
                    ))
                    .power_saver(ReducedEffect::Other(EffectName::RIPPLE)),
                ),
            ],
            frosted: StockEffect::check(
                EffectName::FROSTED,
                EffectSource::new(concat!(
                    include_str!("material.wgsl"),
                    include_str!("frosted.wgsl")
                )),
            ),
            glass: StockEffect::check(
                EffectName::GLASS,
                EffectSource::new(glass_wgsl()).reach(|input| BackdropReach::Lens(input.lens)),
            ),
        }
    }
}

impl<U: EffectParams, K: EffectKind> StockEffect<U, K> {
    /// `source` under `name`, with the module checking it makes.
    ///
    /// # Panics
    ///
    /// When `source` fails its contract.
    fn check(name: EffectName<U, K>, source: EffectSource<U, K>) -> Self {
        let module = source.check().unwrap_or_else(|error| {
            panic!(
                "stock effect `{}` fails its contract: {error:?}",
                name.name()
            )
        });
        Self {
            name,
            source,
            module,
        }
    }
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
