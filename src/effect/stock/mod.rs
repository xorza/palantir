//! Palantir's stock effects: the two press ripples, which every registry holds from its start
//! under names in palantir's reserved prefix, so a theme names them as it names an app's.

use crate::effect::effect_name::EffectName;
use crate::effect::effect_source::{EffectSource, ReducedPaint};
use crate::effect::press_input::PressInput;

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

const fn reserved(name: &str) -> EffectName<PressInput> {
    match EffectName::named(name) {
        Some(name) => name,
        None => panic!("a stock name fits an inline name"),
    }
}

/// How many stock effects a registry holds from its start, at the first ids.
pub(crate) const COUNT: usize = 2;

/// Each stock effect, with its source, in load order: an alternative before what names it.
pub(crate) fn sources() -> [(EffectName<PressInput>, EffectSource<PressInput>); COUNT] {
    [
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
    ]
}
