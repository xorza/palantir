//! The scale the *user* chose, held apart from the scale the *system*
//! reported.

use crate::primitives::approx::EPS;

/// A chrome scale the application chooses, multiplied onto the scale the
/// platform reported — `Display::scale_factor` is the product of the two.
///
/// **Why a second factor rather than one settable number.** The system
/// factor is a fact about the output: it changes when the window moves to
/// another monitor, and nothing in the app should be overwriting it. The
/// user factor is a preference: it survives that move. An app that could
/// write the effective factor would have to re-derive its own preference
/// out of it after every DPI change, and would get it wrong the first
/// time a monitor reported something unexpected. Keeping the two apart is
/// what makes "125% everywhere" mean the same thing on both screens.
///
/// **Every distinct value costs a re-rasterization.** The effective factor
/// reaches the glyph cache key, so each new one mints fresh swash rasters
/// and fresh atlas slots for every glyph on screen — the same cost a
/// monitor move pays. That is fine once, on a preference change, and
/// ruinous sixty times a second. Hence [`Self::LADDER`] and the two step
/// methods: a menu or a pair of shortcuts walks the rungs, and the atlas
/// sees a handful of values over a session. [`Self::new`] accepts any
/// value in range for the app that wants a free one, and pays for it.
///
/// Not serializable on purpose. An app persisting the preference stores
/// the `f32` from [`Self::get`] and reads it back through [`Self::new`],
/// so the range check runs on the way in rather than being derived around.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct UserScale(f32);

impl Default for UserScale {
    fn default() -> Self {
        Self::ONE
    }
}

impl UserScale {
    /// No scaling of its own: the system factor stands alone.
    pub const ONE: Self = Self(1.0);

    /// The rungs [`Self::stepped_up`] and [`Self::stepped_down`] walk, and
    /// the range [`Self::new`] clamps into.
    ///
    /// The set a browser offers, which is the one users already know. It
    /// is denser near `1.0` because that is where a step is a smaller
    /// share of the current size, and so where a coarse rung reads as a
    /// jump.
    pub const LADDER: [f32; 13] = [
        0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
    ];

    /// Smallest value [`Self::new`] accepts.
    pub const MIN: f32 = Self::LADDER[0];

    /// Largest value [`Self::new`] accepts.
    pub const MAX: f32 = Self::LADDER[Self::LADDER.len() - 1];

    /// `factor` clamped to [`Self::MIN`] ..= [`Self::MAX`].
    ///
    /// The clamp is a policy — a UI at 20× is not a UI — so a value
    /// outside the range is answered rather than refused.
    ///
    /// # Panics
    ///
    /// Panics if `factor` is not finite. That is not a value out of
    /// range but a number that never was one, and it would divide every
    /// pointer coordinate and every logical size into nonsense several
    /// layers below here.
    pub fn new(factor: f32) -> Self {
        assert!(
            factor.is_finite(),
            "UserScale::new needs a finite factor, got {factor}",
        );
        Self(factor.clamp(Self::MIN, Self::MAX))
    }

    /// The factor itself, for a caller doing its own arithmetic with it.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The effective factor: `system_scale` scaled by this one.
    ///
    /// **The one definition of the product**, so the windowed host's
    /// per-event copy cannot drift from the one a frame's
    /// [`Display`](crate::Display) is minted with — and a pointer cannot
    /// be divided by a different number than the widgets it is hitting.
    #[inline]
    pub const fn applied_to(self, system_scale: f32) -> f32 {
        system_scale * self.0
    }

    /// The next rung above, or this value when it is already at
    /// [`Self::MAX`].
    ///
    /// Strictly above, so a value between two rungs steps to the higher
    /// of the pair rather than standing still.
    pub fn stepped_up(self) -> Self {
        match Self::LADDER.iter().find(|rung| **rung > self.0 + EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    /// The next rung below, or this value when it is already at
    /// [`Self::MIN`].
    pub fn stepped_down(self) -> Self {
        match Self::LADDER.iter().rev().find(|rung| **rung < self.0 - EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    /// The factor as whole percent, for a menu label — `125` at `1.25`.
    pub fn percent(self) -> u32 {
        (self.0 * 100.0).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_to_the_ladder_ends() {
        assert_eq!(UserScale::new(1.37).get(), 1.37);
        assert_eq!(UserScale::new(12.0).get(), UserScale::MAX);
        assert_eq!(UserScale::new(0.01).get(), UserScale::MIN);
        assert_eq!(UserScale::new(-1.0).get(), UserScale::MIN);
        assert_eq!(UserScale::default(), UserScale::ONE);
    }

    #[test]
    #[should_panic(expected = "UserScale::new needs a finite factor")]
    fn new_rejects_a_non_finite_factor() {
        let _ = UserScale::new(f32::NAN);
    }

    /// The ladder has to be sorted for the two step searches to be the
    /// nearest rung rather than the first one that happened to qualify.
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
        // Two steps up from 1.0 is 1.25, and back down again is 1.0.
        assert_eq!(one.stepped_up().stepped_up().get(), 1.25);
        assert_eq!(one.stepped_up().stepped_up().stepped_down().get(), 1.1);

        let top = UserScale::new(UserScale::MAX);
        assert_eq!(top.stepped_up(), top, "the top rung has nothing above it");
        let bottom = UserScale::new(UserScale::MIN);
        assert_eq!(
            bottom.stepped_down(),
            bottom,
            "the bottom rung has nothing below it",
        );
    }

    /// A value between two rungs must move on the first step, in both
    /// directions — 1.37 sits between 1.25 and 1.5.
    #[test]
    fn stepping_from_between_rungs_lands_on_the_neighbours() {
        let between = UserScale::new(1.37);
        assert_eq!(between.stepped_up().get(), 1.5);
        assert_eq!(between.stepped_down().get(), 1.25);
    }

    #[test]
    fn percent_reads_as_a_label() {
        assert_eq!(UserScale::ONE.percent(), 100);
        assert_eq!(UserScale::new(1.25).percent(), 125);
        assert_eq!(UserScale::new(0.67).percent(), 67);
    }
}
