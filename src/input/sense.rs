use bitflags::bitflags;
use std::time::Duration;

bitflags! {
    /// Which pointer interactions a widget participates in. Widgets
    /// that sense nothing (`Sense::NONE`) are skipped during hit-testing
    /// and clicks/hovers pass through to whatever's beneath.
    ///
    /// Flags compose: `Sense::CLICK | Sense::SCROLL` declares a widget
    /// that captures both clicks and scroll deltas. The "click implies
    /// hover" relationship lives in [`Self::hovers`] — a widget with
    /// `CLICK` set is hoverable regardless of whether `HOVER` is set.
    /// Convention matches egui: containers default to `NONE`, leaf-
    /// interactive widgets pick `CLICK`, draggables add `DRAG`.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct Sense: u8 {
        /// Visible to hover hit-test. Implied by CLICK / DRAG via
        /// [`Self::hovers`]; set explicitly for hover-only widgets
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
        /// Captures wheel/touchpad scroll deltas. Hit-tested
        /// independently of hover/click so a scrollable container
        /// under a clickable child still receives wheel events.
        /// Pinch gestures route on the separate [`Self::PINCH`] bit
        /// so a widget can opt into one without the other.
        const SCROLL = 1 << 3;
        /// Captures touchpad pinch zoom factors. Independent of
        /// `SCROLL` — a graph canvas wanting pan-via-scroll *and*
        /// zoom-via-pinch sets both; a list that scrolls without
        /// reacting to pinch sets only `SCROLL`. Off-target watchers
        /// draw the same line through
        /// [`PointerWake::PINCH`](crate::PointerWake::PINCH).
        const PINCH = 1 << 4;
    }
}

impl Sense {
    /// Ergonomic alias for [`Self::empty`] — the default "inert" sense.
    pub const NONE: Self = Self::empty();

    /// Every pointer interaction, so none reaches widgets underneath.
    ///
    /// The overlay scrim sense: `Popup`'s click-eater and `Modal`'s
    /// backdrop both cover the whole surface to stop clicks, drags,
    /// scrolls and pinches leaking into the `Main` tree — a graph canvas
    /// that pans on middle-drag and zooms on scroll is the case that
    /// makes the last two matter. Named once because two independent
    /// copies of "all four" drift apart the moment a fifth is added.
    ///
    /// Blocks only *routed* input, which is all a sense can reach: it is
    /// the union of four ordinary bits, so any widget wanting all four
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

    /// True if this sense captures scroll deltas.
    pub(crate) const fn scrolls(self) -> bool {
        self.contains(Self::SCROLL)
    }

    /// True if this sense captures pinch zoom factors.
    pub(crate) const fn pinches(self) -> bool {
        self.contains(Self::PINCH)
    }
}

/// Pointer travel from press origin (logical px) before a gesture
/// latches as a drag. Under this, the gesture is still a click. Once
/// crossed, the latch holds for the press lifetime and the release
/// no longer emits a click. Mouse-sized — touch will want larger.
pub(crate) const DRAG_THRESHOLD: f32 = 4.0;

/// Maximum interval between two clicks on the same widget for the
/// second one to be reported as a double-click. 500 ms matches the
/// Windows / Chromium default; macOS's `NSEvent.doubleClickInterval`
/// is user-configurable but defaults to the same neighborhood, and
/// Linux has no system-wide value to read. Tracked per-button on
/// [`crate::input::Capture`].
pub(crate) const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// Maximum pointer travel (logical px) between two clicks for the second
/// to still count as a double-click. A slow drift past this reads as two
/// separate clicks — matches native behaviour and `TextEdit`'s
/// `MULTI_CLICK_RADIUS`. Tracked per-button on [`crate::input::Capture`].
pub(super) const DOUBLE_CLICK_RADIUS: f32 = 5.0;
