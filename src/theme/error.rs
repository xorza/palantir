//! [`ThemeEffectError`]: why [`Ui::set_theme`](crate::Ui::set_theme) refused a theme.

use std::error;
use std::fmt;

use crate::effect::effect_name::EffectName;
use crate::effect::press_input::PressInput;

/// Why a theme was not set: a press slot names an effect it cannot paint. A theme file names an
/// effect by its string and deserializes with no `Ui`, so the set checks the names, as the one
/// door every theme passes. The theme the window held stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeEffectError {
    /// Nothing is loaded under the name.
    Unloaded(EffectName<PressInput>),
    /// The effect under the name takes other parameters than [`PressInput`], or is a backdrop
    /// effect.
    NotPress(EffectName<PressInput>),
}

impl fmt::Display for ThemeEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unloaded(name) => write!(
                f,
                "the theme's press effect `{}` is not loaded",
                name.name()
            ),
            Self::NotPress(name) => write!(
                f,
                "the theme's press effect `{}` is not a color effect over `PressInput`",
                name.name()
            ),
        }
    }
}

impl error::Error for ThemeEffectError {}
