//! Which pointer interactions a widget takes part in — the declaration
//! that decides whether hit-testing considers it at all.

flag_set! {
    /// Which pointer interactions a widget participates in. Widgets
    /// that sense nothing (`Sense::NONE`) are skipped during hit-testing
    /// and clicks/hovers pass through to whatever's beneath.
    ///
    /// Flags compose: `Sense::CLICK | Sense::SCROLL` declares a widget
    /// that captures both clicks and scroll deltas. The "click implies
    /// hover" relationship lives in `Sense::hovers` — a widget with
    /// `CLICK` set is hoverable regardless of whether `HOVER` is set.
    /// Convention matches egui: containers default to `NONE`, leaf-
    /// interactive widgets pick `CLICK`, draggables add `DRAG`.
    pub struct Sense: packed {
        /// Visible to hover hit-test. Implied by CLICK / DRAG via
        /// `Sense::hovers`; set explicitly for hover-only widgets
        /// (tooltip triggers, row highlights) that shouldn't capture
        /// clicks meant for things below.
        const HOVER  = 1 << 0;
        /// Captures press/release. Composes with `HOVER` (implied) and
        /// optionally `DRAG`.
        const CLICK  = 1 << 1;
        /// Participates in threshold-latched drag gestures. Pair with
        /// `CLICK` for click+drag widgets; pair without `CLICK` for
        /// drag-only handles.
        const DRAG   = 1 << 2;
        /// Captures the horizontal half of wheel/touchpad scroll deltas.
        /// Hit-tested independently of hover/click so a scrollable
        /// container under a clickable child still receives wheel events.
        ///
        /// Each axis routes to the topmost row that senses it, so a row
        /// senses only the axes it can pan *this frame*, and the other
        /// axis reaches the container behind it. A row with this bit and
        /// not [`Self::SCROLL_Y`] also takes a plain vertical wheel turn,
        /// as horizontal movement, while no row under the pointer senses
        /// `SCROLL_Y`: a tab strip or a one-line field under a page that
        /// does not scroll.
        const SCROLL_X = 1 << 3;
        /// Captures the vertical half of wheel/touchpad scroll deltas —
        /// [`Self::SCROLL_X`]'s other axis, routed the same way.
        const SCROLL_Y = 1 << 4;
        /// Captures touchpad pinch zoom factors. Independent of
        /// [`Self::SCROLL`] — a graph canvas wanting pan-via-scroll *and*
        /// zoom-via-pinch sets both; a list that scrolls without
        /// reacting to pinch sets only `SCROLL`. Off-target watchers
        /// draw the same line through
        /// [`PointerWake::PINCH`](crate::PointerWake::PINCH).
        const PINCH = 1 << 5;
    }
}

impl Sense {
    /// Wheel/touchpad scroll deltas on both axes: [`Self::SCROLL_X`] and
    /// [`Self::SCROLL_Y`]. Pinch gestures route on the separate
    /// [`Self::PINCH`] bit so a widget can opt into one without the other.
    pub const SCROLL: Self = Self::SCROLL_X.union(Self::SCROLL_Y);

    /// Every pointer interaction, so none reaches widgets underneath.
    ///
    /// The overlay scrim sense: `Popup`'s click-eater and `Modal`'s
    /// backdrop both cover the whole surface to stop clicks, drags,
    /// scrolls and pinches leaking into the `Main` tree — a graph canvas
    /// that pans on middle-drag and zooms on scroll is the case that
    /// makes the last two matter. Named once because two independent
    /// copies of "every routed bit" drift apart the moment one is added —
    /// as the wheel's split into two axes was.
    ///
    /// Blocks only *routed* input, which is all a sense can reach: it is
    /// the union of ordinary bits, so any widget wanting all of them
    /// — a graph canvas — holds it without being a scrim. The watch
    /// streams, which bypass the hit index entirely, are cut off by an
    /// overlay's [`input_scope`](crate::Configure::input_scope) instead.
    pub const ABSORB_POINTER: Self = Self::CLICK
        .union(Self::DRAG)
        .union(Self::SCROLL)
        .union(Self::PINCH);

    /// True if this sense participates in hover hit-test. Any of
    /// `HOVER`/`CLICK`/`DRAG` implies hoverable; `SCROLL`-only widgets
    /// are invisible to the hover layer so the cursor / tooltip keeps
    /// targeting content beneath.
    pub(crate) const fn hovers(self) -> bool {
        self.intersects(Self::HOVER.union(Self::CLICK).union(Self::DRAG))
    }

    /// True if this sense captures press/release. `CLICK` and `DRAG`
    /// both qualify — drag widgets must capture the press to set
    /// `active` and start tracking pointer travel.
    pub(crate) const fn clicks(self) -> bool {
        self.intersects(Self::CLICK.union(Self::DRAG))
    }

    /// True if this sense captures pinch zoom factors.
    pub(crate) const fn pinches(self) -> bool {
        self.contains(Self::PINCH)
    }
}

#[cfg(test)]
mod tests {
    use crate::input::sense::Sense;

    /// The two derived predicates read the union they document, not a
    /// single bit — `SCROLL`-only stays invisible to the hover layer.
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
