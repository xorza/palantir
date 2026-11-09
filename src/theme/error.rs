//! [`ThemeEffectError`]: why [`Ui::set_theme`](crate::Ui::set_theme) refused a theme.

use std::error;
use std::fmt;

use crate::effect::press_input::PressInput;
use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::paint::material::material_input::MaterialInput;

/// Why a theme was not set: a press slot or a chrome's material names an effect it cannot paint.
/// A theme file names an effect by its string and deserializes with no `Ui`, so the set checks the
/// names, as the one door every theme passes. The theme the window held stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeEffectError {
    /// Nothing is loaded under a press slot's name.
    UnloadedPress(EffectName<PressInput>),
    /// The effect under a press slot's name takes other parameters than [`PressInput`], or is a
    /// backdrop effect.
    NotPress(EffectName<PressInput>),
    /// Nothing is loaded under a material's name.
    UnloadedMaterial(EffectName<MaterialInput, BackdropEffect>),
    /// The effect under a material's name takes other parameters than [`MaterialInput`], or is a
    /// color effect.
    NotMaterial(EffectName<MaterialInput, BackdropEffect>),
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
                "the theme's press effect `{}` is not a color effect over `PressInput`",
                name.name()
            ),
            Self::UnloadedMaterial(name) => write!(
                f,
                "the theme's material effect `{}` is not loaded",
                name.name()
            ),
            Self::NotMaterial(name) => write!(
                f,
                "the theme's material effect `{}` is not a backdrop effect over `MaterialInput`",
                name.name()
            ),
        }
    }
}

impl error::Error for ThemeEffectError {}
