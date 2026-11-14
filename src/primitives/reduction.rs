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

    /// Whether this is [`Self::Reduced`].
    pub const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

/// Whether transparency, motion and power use run whole or reduced, the three reductions palantir
/// follows, in the order an effect's alternative for one wins over the next. The one form of the
/// three: the system's reports ([`Ui::system_reductions`](crate::Ui::system_reductions)), the
/// resolved settings and each frame's ([`Ui::reductions`](crate::Ui::reductions)). Plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reductions {
    /// Reduced, no capture, blur or effect runs, as CSS `prefers-reduced-transparency` asks, and
    /// every [`Material`](crate::Material)'s box draws as a plain box of its
    /// [`fallback`](crate::Material::fallback) in place of its fill: by default
    /// [`Theme::material_fallback`](crate::Theme::material_fallback), the theme's float surface,
    /// translucent and readable. Under reduced power the system's answer reads as reduced, as
    /// Windows' Energy saver turns transparency effects off, so only a pick by hand keeps
    /// transparency whole.
    pub transparency: Reduction,
    /// Reduced, every [`Ui::animate`](crate::Ui::animate) and theme transition snaps to its
    /// target, as CSS `prefers-reduced-motion` asks of transitions, and a
    /// [decorative](crate::MotionRole::Decorative) paint animation holds still; an
    /// [essential](crate::MotionRole::Essential) one, as a spinner's turn or a caret's
    /// blink, still runs, as WCAG 2.3.3 lets essential motion stay
    /// (<https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions>). Under reduced
    /// power with motion that follows the system, the transitions snap too, and paint loops run
    /// on.
    pub motion: Reduction,
    /// Reduced, as macOS Low Power Mode, Windows' Energy saver or battery saver, or Linux's
    /// power-saver profile reports it: the [frame rate](crate::Ui::is_frame_rate_reduced) and
    /// transparency reduce, every [`Ui::animate`](crate::Ui::animate) and theme transition snaps
    /// where its preference follows the system, paint loops run on, a move records every window
    /// again, and an app reads it to swap to cheaper paint, as Apple asks an app to reduce its
    /// activity in Low Power Mode
    /// (<https://developer.apple.com/documentation/foundation/processinfo/islowpowermodeenabled>).
    pub power: Reduction,
}

impl Reductions {
    /// Nothing reduced.
    pub const NONE: Self = Self {
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
