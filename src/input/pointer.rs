//! Raw pointer vocabulary: [`PointerButton`] and the [`PointerEvent`] stream watchers read.

use glam::Vec2;

/// Which pointer button an event came from; the discriminants index the
/// matching [`ButtonState`](crate::ButtonState) slots on [`ResponseState`](crate::ResponseState).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PointerButton {
    /// Primary button: clicks, drags, focus.
    Left = 0,
    /// Secondary button; the context-menu trigger.
    Right = 1,
    /// Wheel button.
    Middle = 2,
}

impl PointerButton {
    pub(crate) const COUNT: usize = 3;

    pub(crate) const ALL: [Self; Self::COUNT] = [Self::Left, Self::Right, Self::Middle];

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}

const _: () = {
    let mut i = 0;
    while i < PointerButton::COUNT {
        assert!(
            PointerButton::ALL[i].idx() == i,
            "PointerButton::ALL must list every discriminant in order",
        );
        i += 1;
    }
    assert!(PointerButton::Middle.idx() + 1 == PointerButton::COUNT);
};

/// Unified pointer event stream, populated when the matching
/// [`PointerWake`](crate::PointerWake) flag is set. No "click": that is
/// per-widget logic ([`ButtonState::clicked`](crate::ButtonState::clicked)). An
/// overlay scrim empties it for the layers below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved; gated on [`PointerWake::MOVE`](crate::PointerWake::MOVE).
    Move(Vec2),
    /// Button pressed; gated on [`PointerWake::BUTTONS`](crate::PointerWake::BUTTONS).
    Down {
        /// Cursor position, logical px.
        pos: Vec2,
        /// Which button went down.
        button: PointerButton,
    },
    /// Button released; gated like `Down`.
    Up {
        /// Cursor position, logical px.
        pos: Vec2,
        /// Which button came up.
        button: PointerButton,
    },
    /// Wheel or touchpad scroll; gated on [`PointerWake::SCROLL`](crate::input::watch::PointerWake::SCROLL).
    Scroll {
        /// Cursor position, logical px.
        pos: Vec2,
        /// Pixel-precise delta, as touchpads report it.
        pixels: Vec2,
        /// Notched wheel ticks.
        lines: Vec2,
    },
    /// Pinch-zoom; gated on [`PointerWake::PINCH`](crate::input::watch::PointerWake::PINCH).
    Zoom {
        /// Pinch centroid, logical px.
        pos: Vec2,
        /// Multiplicative zoom delta; `1.0` is no change.
        factor: f32,
    },
    /// Pointer left the surface; no position.
    Leave,
}
