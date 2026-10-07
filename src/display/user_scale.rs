//! The scale the *user* chose, held apart from the scale the *system* reported.

use crate::primitives::math::domain::EPS;

/// A chrome scale the application chooses, multiplied onto the platform's;
/// `Display::scale_factor` is the product.
///
/// **A second factor, not one settable number:** the system factor is a fact
/// about the output and changes when the window moves monitors; the user factor
/// is a preference that survives that move. Keeping them apart makes "125%
/// everywhere" mean the same on both screens.
///
/// **Every distinct value costs a re-rasterization:** the effective factor
/// reaches the glyph cache key, so each new one mints fresh rasters and atlas
/// slots for every glyph on screen. Fine once, ruinous sixty times a second.
/// Hence [`Self::LADDER`] and the step methods; [`Self::new`] accepts any value
/// in range for apps that want a free one.
///
/// Not serializable on purpose: persist the `f32` from [`Self::get`] and read it
/// back through [`Self::new`] so the range check runs. A non-number comes back
/// as `None`; `UserScale::new(saved).unwrap_or_default()` falls back to
/// [`Self::ONE`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[must_use]
pub struct UserScale(f32);

impl Default for UserScale {
    fn default() -> Self {
        Self::ONE
    }
}

impl UserScale {
    /// No scaling.
    pub const ONE: Self = Self(1.0);

    /// The rungs [`Self::stepped_up`] and [`Self::stepped_down`] walk, and the
    /// range [`Self::new`] clamps into. The set a browser offers, denser near
    /// `1.0` where a coarse rung reads as a jump.
    pub const LADDER: [f32; 13] = [
        0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
    ];

    /// Smallest value [`Self::new`] yields.
    pub const MIN: f32 = Self::LADDER[0];

    /// Largest value [`Self::new`] yields.
    pub const MAX: f32 = Self::LADDER[Self::LADDER.len() - 1];

    /// `factor` clamped to [`Self::MIN`] ..= [`Self::MAX`], or `None` when not
    /// finite.
    ///
    /// Out-of-range values are clamped (a UI at 20x is not a UI); a non-finite
    /// factor is refused, as it would corrupt every pointer coordinate and logical
    /// size downstream.
    pub const fn new(factor: f32) -> Option<Self> {
        if factor.is_finite() {
            Some(Self(factor.clamp(Self::MIN, Self::MAX)))
        } else {
            None
        }
    }

    #[inline]
    /// The scale factor.
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The effective factor: `system_scale` scaled by this one. The one definition
    /// of the product, so the host's per-event copy and a frame's
    /// [`Display`](crate::Display) can't disagree.
    #[inline]
    pub const fn applied_to(self, system_scale: f32) -> f32 {
        system_scale * self.0
    }

    /// The next rung above, or this value at [`Self::MAX`]. Strictly above, so a
    /// value between rungs steps to the higher.
    pub fn stepped_up(self) -> Self {
        match Self::LADDER.iter().find(|rung| **rung > self.0 + EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    /// The next rung below, or this value at [`Self::MIN`].
    pub fn stepped_down(self) -> Self {
        match Self::LADDER.iter().rev().find(|rung| **rung < self.0 - EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "a user scale is a positive factor, so its percent is positive"
    )]
    /// The scale as a whole percent.
    pub const fn percent(self) -> u32 {
        (self.0 * 100.0).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In range stands, out of range clamps, and only a non-number is refused,
    /// infinities included: clamping one would pass a corrupt preference off as a
    /// choice.
    #[test]
    fn new_clamps_to_the_ladder_ends_and_refuses_non_numbers() {
        for (factor, want) in [
            (1.37, Some(1.37)),
            (12.0, Some(UserScale::MAX)),
            (0.01, Some(UserScale::MIN)),
            (-1.0, Some(UserScale::MIN)),
            (f32::NAN, None),
            (f32::INFINITY, None),
            (f32::NEG_INFINITY, None),
        ] {
            assert_eq!(UserScale::new(factor).map(UserScale::get), want, "{factor}");
        }
        assert_eq!(UserScale::default(), UserScale::ONE);
    }

    /// The ladder must be sorted for the step searches to find the nearest rung.
    #[test]
    fn the_ladder_ascends_and_holds_one() {
        assert!(UserScale::LADDER.is_sorted_by(|a, b| a < b));
        assert!(UserScale::LADDER.contains(&1.0));
    }

    #[test]
    fn stepping_walks_the_rungs_and_stops_at_the_ends() {
        // 1.0 is rung 5, so up is 1.1 and down is 0.9.
        let one = UserScale::ONE;
        assert_eq!(one.stepped_up().get(), 1.1);
        assert_eq!(one.stepped_down().get(), 0.9);
        // Two steps up from 1.0 is 1.25, and one back down is 1.1.
        assert_eq!(one.stepped_up().stepped_up().get(), 1.25);
        assert_eq!(one.stepped_up().stepped_up().stepped_down().get(), 1.1);

        let top = UserScale::new(UserScale::MAX).unwrap();
        assert_eq!(top.stepped_up(), top, "the top rung has nothing above it");
        let bottom = UserScale::new(UserScale::MIN).unwrap();
        assert_eq!(
            bottom.stepped_down(),
            bottom,
            "the bottom rung has nothing below it",
        );
    }

    /// A value between two rungs moves on the first step in both directions
    /// (1.37 sits between 1.25 and 1.5). Within `EPS` of a rung it counts as on
    /// it; past `EPS` above, a step down lands on it.
    #[test]
    fn stepping_from_between_rungs_lands_on_the_neighbours() {
        for (factor, up, down) in [
            (1.37, 1.5, 1.25),
            (1.0 + EPS * 0.5, 1.1, 0.9),
            (1.0 - EPS * 0.5, 1.1, 0.9),
            (1.0 + EPS * 2.0, 1.1, 1.0),
            (1.0 - EPS * 2.0, 1.0, 0.9),
        ] {
            let scale = UserScale::new(factor).unwrap();
            assert_eq!(scale.stepped_up().get(), up, "{factor} up");
            assert_eq!(scale.stepped_down().get(), down, "{factor} down");
        }
    }

    #[test]
    fn percent_reads_as_a_label() {
        assert_eq!(UserScale::ONE.percent(), 100);
        assert_eq!(UserScale::new(1.25).unwrap().percent(), 125);
        assert_eq!(UserScale::new(0.67).unwrap().percent(), 67);
    }
}
