//! Pointer event taxonomy: the [`PointerButton`] enum identifying
//! which mouse / touchpad button fired, and the unified
//! [`PointerEvent`] stream watchers read from
//! [`InputState::frame_pointer_events`](crate::input::input_state::InputState).
//!
//! Wake-gate flags live in
//! [`watches::PointerWake`](crate::input::watch::PointerWake);
//! per-widget hit-test routing lives in
//! [`sense::Sense`](crate::input::sense::Sense). This module is the raw
//! event vocabulary — no routing logic.

use glam::Vec2;
use strum::{EnumCount, EnumIter, IntoEnumIterator};

/// Which pointer button an event came from. The discriminants are the
/// indices of the matching [`ButtonState`](crate::ButtonState) slots on
/// [`ResponseState`](crate::ResponseState), so the three buttons get an
/// identical query surface — middle-click is as queryable as left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumCount, EnumIter)]
#[repr(u8)]
pub enum PointerButton {
    /// Primary button. Drives clicks, drags, and focus.
    Left = 0,
    /// Secondary button. `right.clicked()` is the context-menu trigger.
    Right = 1,
    /// Wheel button.
    Middle = 2,
}

impl PointerButton {
    /// Iterate every variant in declaration order. Wraps
    /// `strum::IntoEnumIterator` so callers don't need to bring the
    /// trait into scope.
    ///
    /// Widget code reaches for this when a rule is "any button" rather
    /// than a named one — `Popup`'s outside-click dismissal — so that
    /// adding a fourth button doesn't leave a hand-written
    /// `left || right || middle` silently short.
    #[inline]
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }

    #[inline]
    pub(super) fn idx(self) -> usize {
        self as usize
    }
}

/// Unified pointer event stream populated when the matching
/// [`PointerWake`](crate::PointerWake) flag is set. Each variant is the raw
/// event — "click" is intentionally absent: it's per-widget logic already
/// routed through capture into
/// [`ButtonState::clicked`](crate::ButtonState::clicked).
///
/// Sibling of [`KeyPress`](crate::KeyPress) — both live in their own
/// module so the raw-event taxonomy is in one place;
/// [`PointerWake`](crate::PointerWake) and
/// [`KeyboardWake`](crate::KeyboardWake) provide the wake-gate flags.
/// The two streams are read through the same layer gate as well: an
/// overlay scrim empties
/// [`Ui::pointer_events`](crate::Ui::pointer_events) for the layers
/// below it, just as a keyboard capture does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved to `pos` (logical pixels). Gated on
    /// [`PointerWake::MOVE`](crate::PointerWake::MOVE).
    Move(Vec2),
    /// Button pressed at `pos`. Gated on
    /// [`PointerWake::BUTTONS`](crate::PointerWake::BUTTONS).
    /// Hit-test + capture routing happens independently; a watcher
    /// that can see the stream at all sees every press regardless of
    /// where it landed.
    Down {
        /// Cursor position in logical pixels, surface space.
        pos: Vec2,
        /// Which button went down.
        button: PointerButton,
    },
    /// Button released at `pos`. Same gating + routing as `Down`.
    Up {
        /// Cursor position in logical pixels, surface space.
        pos: Vec2,
        /// Which button came up.
        button: PointerButton,
    },
    /// Wheel / touchpad scroll at `pos`. `pixels` is pixel-precise
    /// touchpad deltas; `lines` is notched wheel ticks. One or both
    /// may be non-zero per event. Gated on
    /// [`PointerWake::SCROLL`](crate::input::watch::PointerWake::SCROLL).
    Scroll {
        /// Cursor position in logical pixels, surface space.
        pos: Vec2,
        /// Pixel-precise delta, as touchpads report it.
        pixels: Vec2,
        /// Notched wheel ticks. Use this for "one wheel notch" intent
        /// (zoom by steps) rather than deriving it from `pixels`.
        lines: Vec2,
    },
    /// Pinch-zoom factor at `pos`. `factor` is the multiplicative
    /// delta (1.0 = no zoom). Gated on
    /// [`PointerWake::PINCH`](crate::input::watch::PointerWake::PINCH) —
    /// not `SCROLL`, so watching wheel ticks doesn't also wake on pinch.
    Zoom {
        /// Pinch centroid in logical pixels, surface space.
        pos: Vec2,
        /// Multiplicative zoom delta for this event; `1.0` is no change.
        factor: f32,
    },
    /// Pointer left the surface. No position — by the time this
    /// fires there isn't one. Emitted when any pointer-class
    /// watch is active so watchers can clean up.
    Leave,
}
