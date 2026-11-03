//! Whether a person wants an effect the system can ask to reduce: motion or transparency.

/// Follow the system's ask to reduce an effect, or hold it by hand, as
/// [`SchemePreference`](crate::SchemePreference) does for the scheme: one type for the two
/// reductions palantir follows, motion ([`Ui::set_motion_preference`](crate::Ui::set_motion_preference))
/// and transparency ([`Ui::set_transparency_preference`](crate::Ui::set_transparency_preference)),
/// as CSS names `prefers-reduced-motion` and `prefers-reduced-transparency` alike. The system's
/// answer is the default and the app's pick overrides it, as an app's own setting overrides the
/// OS's dark mode.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ReducePreference {
    /// Reduced exactly when the system asks; full where nothing reports.
    #[default]
    System,
    /// Full, whatever the system asks.
    Full,
    /// Reduced, whatever the system asks.
    Reduced,
}

impl ReducePreference {
    /// The effect this preference shows when the system asks for `system`: a pick by hand wins.
    pub(crate) const fn resolve(self, system: Reduction) -> Reduction {
        match self {
            Self::System => system,
            Self::Full => Reduction::Full,
            Self::Reduced => Reduction::Reduced,
        }
    }
}

/// Whether an effect runs whole or reduced, as the system asks or a preference resolves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Reduction {
    /// The effect runs whole.
    #[default]
    Full,
    /// The effect is reduced.
    Reduced,
}

impl From<bool> for Reduction {
    /// `true` is [`Self::Reduced`], as a system reports its setting.
    fn from(reduced: bool) -> Self {
        if reduced { Self::Reduced } else { Self::Full }
    }
}

impl Reduction {
    pub(crate) const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::reduce_preference::{ReducePreference, Reduction};

    /// Every preference against every system answer: a pick by hand wins, `System` follows.
    #[test]
    fn a_pick_wins_and_system_follows() {
        use Reduction::{Full, Reduced};
        for (preference, system, want) in [
            (ReducePreference::System, Full, Full),
            (ReducePreference::System, Reduced, Reduced),
            (ReducePreference::Full, Full, Full),
            (ReducePreference::Full, Reduced, Full),
            (ReducePreference::Reduced, Full, Reduced),
            (ReducePreference::Reduced, Reduced, Reduced),
        ] {
            assert_eq!(
                preference.resolve(system),
                want,
                "{preference:?} over {system:?}"
            );
        }
        assert_eq!(ReducePreference::default(), ReducePreference::System);
        assert_eq!(Reduction::from(true), Reduced);
        assert_eq!(Reduction::from(false), Full);
    }
}
