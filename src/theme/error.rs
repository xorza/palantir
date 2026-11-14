//! [`ThemeEffectError`]: why [`Ui::set_scheme_theme`](crate::Ui::set_scheme_theme) refused a theme.

use std::error;
use std::fmt;

use crate::effect::press_params::PressParams;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::paint::material::material_effect::MaterialEffect;

/// Why a theme was not set: a press slot or a chrome's material names an effect it cannot paint.
/// A theme file names an effect by its string and deserializes with no `Ui`, so the set checks the
/// names, as the one door every theme passes. The theme the window held stays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThemeEffectError {
    /// Nothing is loaded under a press slot's name.
    UnloadedPress(EffectName<PressParams>),
    /// The effect under a press slot's name takes other parameters than [`PressParams`], or is a
    /// backdrop effect.
    NotPress(EffectName<PressParams>),
    /// Nothing is loaded under the name of a material's effect.
    UnloadedMaterial(MaterialEffect),
    /// The effect under a material's name is no material's: a color effect, one that binds
    /// images, or one over other than f32 lanes that fit
    /// [`MaterialEffect::MAX_LANES`](crate::MaterialEffect::MAX_LANES).
    NotMaterial(MaterialEffect),
    /// A material holds another count of parameter lanes than its effect takes.
    MaterialParams {
        /// The material's effect, with the lanes it holds.
        effect: MaterialEffect,
        /// The lanes the effect takes.
        want: u8,
        /// The lanes the material holds.
        got: u8,
    },
}

impl fmt::Display for ThemeEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnloadedPress(name) => write!(
                f,
                "the theme's press effect `{}` is not loaded",
                name.name()
            ),
            Self::NotPress(name) => write!(
                f,
                "the theme's press effect `{}` is not a color effect over `PressParams`",
                name.name()
            ),
            Self::UnloadedMaterial(effect) => write!(
                f,
                "the theme's material effect `{}` is not loaded",
                effect.name()
            ),
            Self::NotMaterial(effect) => write!(
                f,
                "the theme's material effect `{}` is not a backdrop effect over at most 8 f32 \
                 lanes that binds no image",
                effect.name()
            ),
            Self::MaterialParams { effect, want, got } => write!(
                f,
                "the theme's material effect `{}` takes {want} parameter lanes, and its material \
                 holds {got}",
                effect.name()
            ),
        }
    }
}

impl error::Error for ThemeEffectError {}
