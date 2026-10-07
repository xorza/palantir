//! Which pointer interactions a widget takes part in — the declaration
//! that decides whether hit-testing considers it at all.

flag_set! {
    /// Which pointer interactions a widget participates in. `NONE` is
    /// skipped by hit-testing, so input passes through to what is beneath.
    /// `CLICK` and `DRAG` imply hover (see `Sense::hovers`).
    pub struct Sense: packed {
        /// Visible to the hover hit-test; implied by `CLICK` and `DRAG`.
        const HOVER  = 1 << 0;
        /// Captures press and release.
        const CLICK  = 1 << 1;
        /// Threshold-latched drag gestures.
        const DRAG   = 1 << 2;
        /// Horizontal wheel/touchpad scroll deltas.
        ///
        /// Each axis routes to the topmost row that senses it. A row with this
        /// bit and not [`Self::SCROLL_Y`] also takes a plain vertical wheel
        /// turn while no row under the pointer senses `SCROLL_Y`.
        const SCROLL_X = 1 << 3;
        /// Vertical scroll deltas, routed like [`Self::SCROLL_X`].
        const SCROLL_Y = 1 << 4;
        /// Touchpad pinch zoom factors, independent of [`Self::SCROLL`].
        /// Off-target watchers use [`PointerWake::PINCH`](crate::PointerWake::PINCH).
        const PINCH = 1 << 5;
    }
}

impl Sense {
    /// Scroll on both axes: [`Self::SCROLL_X`] and [`Self::SCROLL_Y`].
    pub const SCROLL: Self = Self::SCROLL_X.union(Self::SCROLL_Y);

    /// Every routed pointer interaction, so none reaches widgets underneath.
    ///
    /// The scrim sense of `Popup` and `Modal`. Watch streams bypass the hit
    /// index; an overlay's [`input_scope`](crate::Configure::input_scope) cuts those off.
    pub const ABSORB_POINTER: Self = Self::CLICK
        .union(Self::DRAG)
        .union(Self::SCROLL)
        .union(Self::PINCH);

    /// `HOVER`, `CLICK` or `DRAG`; scroll-only widgets stay invisible to hover.
    pub(crate) const fn hovers(self) -> bool {
        self.intersects(Self::HOVER.union(Self::CLICK).union(Self::DRAG))
    }

    /// `CLICK` or `DRAG`: drag widgets must capture the press to start tracking.
    pub(crate) const fn clicks(self) -> bool {
        self.intersects(Self::CLICK.union(Self::DRAG))
    }

    /// Whether this sense captures pinch zoom factors.
    pub(crate) const fn pinches(self) -> bool {
        self.contains(Self::PINCH)
    }
}

#[cfg(test)]
mod tests {
    use crate::input::sense::Sense;

    /// `SCROLL`-only stays invisible to the hover layer.
    #[test]
    fn hover_and_click_predicates_span_their_documented_unions() {
        for sense in [Sense::HOVER, Sense::CLICK, Sense::DRAG] {
            assert!(sense.hovers(), "{sense:?} should hover");
        }
        assert!(!Sense::SCROLL.hovers());
        assert_eq!(Sense::SCROLL, Sense::SCROLL_X | Sense::SCROLL_Y);
        assert!(!Sense::PINCH.hovers());
        assert!(!Sense::NONE.hovers());

        assert!(Sense::CLICK.clicks());
        assert!(Sense::DRAG.clicks());
        assert!(!Sense::HOVER.clicks());

        assert!(Sense::ABSORB_POINTER.contains(Sense::CLICK));
        assert!(Sense::ABSORB_POINTER.contains(Sense::PINCH));
        assert!(Sense::ABSORB_POINTER.contains(Sense::SCROLL));
        assert!(
            !Sense::ABSORB_POINTER.contains(Sense::HOVER),
            "the scrim is the routed bits, not every bit",
        );
    }
}
