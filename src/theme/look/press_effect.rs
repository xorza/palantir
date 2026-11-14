//! [`PressEffect`]: the effect a themed widget draws for each press.

use std::time::Duration;

use crate::effect::press_params::PressParams;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::paint::color::RgbaF32;

/// The effect a themed widget draws over its chrome and under its content for each press, as
/// Compose's theme supplies its `Indication` to every clickable: the effect, the colors it reads,
/// and how long a press takes to grow in and to fade out ([`PressParams`]). Each scheme's theme
/// holds its own, so one effect serves both schemes through the colors, and a theme transition
/// blends them. Refused: the scheme in the effect's input, which flips once where a transition
/// blends, and which no platform hands a shader (Android's `RippleDrawable` and Compose's ripple
/// take their colors from the theme).
///
/// A widget keeps at most [`Self::MAX_PRESSES`] live; a press past them starts none. A theme file
/// names the effect by its string, which [`Ui::set_theme`](crate::Ui::set_theme) checks against
/// the loaded effects; a theme transition snaps it, as two effects do not blend. A slot with none
/// holds `None` ([`SlotDefaults::press`](crate::SlotDefaults::press)), so no press effect lacks
/// its effect.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressEffect {
    /// The color effect each press draws.
    pub effect: EffectName<PressParams>,
    /// The color the effect reads, linear.
    pub color: RgbaF32,
    /// A second color the effect reads, linear: the sparkles of
    /// [`EffectName::SPARKLE`](crate::EffectName::SPARKLE), as Android's
    /// `RippleDrawable::setEffectColor` sets them. Android's default is white at α 141/255, which
    /// reads on a dark ground; a light theme takes a dark one. An effect may read none of it, as
    /// the flat ripple does.
    pub highlight: RgbaF32,
    /// How long a press takes to grow in.
    #[serde(with = "crate::theme::duration_codec::seconds")]
    pub enter: Duration,
    /// How long it takes to fade out after the release.
    #[serde(with = "crate::theme::duration_codec::seconds")]
    pub exit: Duration,
}

impl PressEffect {
    /// The most presses a widget draws at once, as Android's `RippleDrawable` holds at most
    /// `MAX_RIPPLES`, 10.
    pub const MAX_PRESSES: usize = 10;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ron::ser;

    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::paint::color::RgbaF32;
    use crate::theme::look::press_effect::PressEffect;

    /// A theme file names a press effect by its string and writes its times in seconds, and reads
    /// back the same slot; a file that leaves a field out reads none.
    #[test]
    fn a_theme_file_names_the_press_effect_by_its_string() {
        let press = PressEffect {
            effect: EffectName::RIPPLE,
            color: RgbaF32::new(1.0, 1.0, 1.0, 0.25),
            highlight: RgbaF32::new(0.0, 0.0, 0.0, 0.5),
            enter: Duration::from_millis(225),
            exit: Duration::from_millis(150),
        };
        let text = ser::to_string(&press).expect("serialize");
        assert_eq!(
            text,
            r#"(effect:"palantir.ripple",color:"color(srgb-linear 1 1 1 / 0.25)",highlight:"color(srgb-linear 0 0 0 / 0.5)",enter:0.225,exit:0.15)"#
        );
        let back: PressEffect = ron::from_str(&text).expect("parse");
        assert_eq!(back, press);
        let short = ron::from_str::<PressEffect>(r#"(effect:"palantir.ripple")"#);
        assert!(short.is_err(), "{short:?}");
    }
}
