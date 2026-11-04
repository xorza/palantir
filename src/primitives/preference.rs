//! [`Preference`]: a person's pick of a setting the system also reports.

/// Follow what the system reports for a setting, or hold one value by hand, as egui's
/// `ThemePreference` and Zed's `theme.mode` offer the scheme
/// (<https://docs.rs/egui/latest/egui/enum.ThemePreference.html>,
/// <https://zed.dev/docs/appearance>) and an app's own "Reduce motion" setting overrides the
/// OS's. One shape for every setting palantir follows from the system, gathered in
/// [`UiPreferences`](crate::UiPreferences). Refused: an enum of its own for each setting, the same
/// three states spelled twice.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Preference<T> {
    /// What the system reports.
    #[default]
    System,
    /// This value, whatever the system reports.
    Fixed(T),
}

impl<T: Copy> Preference<T> {
    /// The value this preference shows when the system reports `system`: a pick by hand wins.
    pub(crate) const fn resolve(self, system: T) -> T {
        match self {
            Self::System => system,
            Self::Fixed(value) => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::preference::Preference;

    /// Every preference against two system answers: a pick by hand wins, `System` follows.
    #[test]
    fn a_pick_wins_and_system_follows() {
        for (preference, system, want) in [
            (Preference::System, 1, 1),
            (Preference::System, 2, 2),
            (Preference::Fixed(3), 1, 3),
            (Preference::Fixed(3), 3, 3),
        ] {
            assert_eq!(
                preference.resolve(system),
                want,
                "{preference:?} over {system}"
            );
        }
        assert_eq!(Preference::<u8>::default(), Preference::System);
    }
}
