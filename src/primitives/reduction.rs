//! [`Reduction`]: whether an effect the system can ask to reduce runs whole.

/// Whether an effect runs whole or reduced, as the system asks or a [`Preference`] holds: motion,
/// transparency or power use, as CSS names `prefers-reduced-motion` and
/// `prefers-reduced-transparency` alike.
///
/// [`Preference`]: crate::Preference
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Reduction {
    /// The effect runs whole.
    #[default]
    Full,
    /// The effect is reduced.
    Reduced,
}

impl Reduction {
    /// `true` is [`Self::Reduced`], as a system reports its setting.
    pub(crate) const fn of(reduced: bool) -> Self {
        if reduced { Self::Reduced } else { Self::Full }
    }

    pub(crate) const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::reduction::Reduction;

    #[test]
    fn a_system_flag_reads_as_its_reduction() {
        assert_eq!(Reduction::of(true), Reduction::Reduced);
        assert_eq!(Reduction::of(false), Reduction::Full);
        assert!(Reduction::Reduced.is_reduced());
        assert!(!Reduction::Full.is_reduced());
    }
}
