//! [`Reduction`]: whether an effect the system can ask to reduce runs whole, and [`Reductions`],
//! the three palantir follows.

/// Whether an effect runs whole or reduced, as the system asks or a [`Preference`] holds: motion,
/// transparency or power use, as CSS names `prefers-reduced-motion` and
/// `prefers-reduced-transparency` alike.
///
/// [`Preference`]: crate::Preference
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum Reduction {
    /// The effect runs whole. Refused: `Full`, which reads as "fully reduced"; `None`, which
    /// shadows `Option::None` under a glob import.
    #[default]
    Unreduced,
    /// The effect is reduced.
    Reduced,
}

impl Reduction {
    /// `true` is [`Self::Reduced`], as a system reports its setting.
    pub(crate) const fn of(reduced: bool) -> Self {
        if reduced {
            Self::Reduced
        } else {
            Self::Unreduced
        }
    }

    pub(crate) const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

/// Whether transparency, motion and power use run whole or reduced, the three reductions palantir
/// follows, in the order an effect's alternative for one wins over the next. The one form of the
/// three: the system's reports, the resolved settings and each frame's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reductions {
    pub(crate) transparency: Reduction,
    pub(crate) motion: Reduction,
    pub(crate) power: Reduction,
}

impl Reductions {
    /// Nothing reduced.
    pub(crate) const NONE: Self = Self {
        transparency: Reduction::Unreduced,
        motion: Reduction::Unreduced,
        power: Reduction::Unreduced,
    };
}

#[cfg(test)]
mod tests {
    use crate::primitives::reduction::Reduction;

    #[test]
    fn a_system_flag_reads_as_its_reduction() {
        assert_eq!(Reduction::of(true), Reduction::Reduced);
        assert_eq!(Reduction::of(false), Reduction::Unreduced);
        assert!(Reduction::Reduced.is_reduced());
        assert!(!Reduction::Unreduced.is_reduced());
    }
}
