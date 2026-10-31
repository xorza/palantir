//! Which scheme a person asked for: the system's, or one of the two by hand.

use crate::theme::color_scheme::ColorScheme;

/// The scheme a person picked: follow the system, or hold light or dark, as egui's
/// `ThemePreference` and Zed's `theme.mode` offer the three
/// (<https://docs.rs/egui/latest/egui/enum.ThemePreference.html>,
/// <https://zed.dev/docs/appearance>). Palantir resolves it against the system's report, so an
/// app sets a theme per scheme once; refused: iced's shape, where each app subscribes to the
/// system and swaps its own theme.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SchemePreference {
    /// The scheme the system reports, or [`Self::FALLBACK`] where nothing reports one.
    #[default]
    System,
    /// Light, whatever the system reports.
    Light,
    /// Dark, whatever the system reports.
    Dark,
}

impl SchemePreference {
    /// The scheme [`Self::System`] shows where nothing reports one (a harness, an offscreen host,
    /// a desktop with no reader): dark, the look palantir had before it read the system.
    pub const FALLBACK: ColorScheme = ColorScheme::Dark;

    /// The scheme this preference shows when the system reports `system`: a pick by hand wins,
    /// then the system, then [`Self::FALLBACK`].
    pub(crate) const fn resolve(self, system: Option<ColorScheme>) -> ColorScheme {
        match (self, system) {
            (Self::Light, _) => ColorScheme::Light,
            (Self::Dark, _) => ColorScheme::Dark,
            (Self::System, Some(scheme)) => scheme,
            (Self::System, None) => Self::FALLBACK,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::color_scheme::ColorScheme::{Dark, Light};
    use crate::theme::scheme_preference::SchemePreference;

    /// Every preference against every system answer: a pick by hand ignores the system, `System`
    /// follows it, and with no answer falls back to dark.
    #[test]
    fn a_preference_resolves_against_every_system_answer() {
        let table = [
            (SchemePreference::System, None, Dark),
            (SchemePreference::System, Some(Light), Light),
            (SchemePreference::System, Some(Dark), Dark),
            (SchemePreference::Light, None, Light),
            (SchemePreference::Light, Some(Light), Light),
            (SchemePreference::Light, Some(Dark), Light),
            (SchemePreference::Dark, None, Dark),
            (SchemePreference::Dark, Some(Light), Dark),
            (SchemePreference::Dark, Some(Dark), Dark),
        ];
        for (preference, system, want) in table {
            assert_eq!(
                preference.resolve(system),
                want,
                "{preference:?} with the system at {system:?}"
            );
        }
        assert_eq!(SchemePreference::default(), SchemePreference::System);
    }
}
