//! Pointer event taxonomy: the [`PointerButton`] enum identifying
//! which mouse / touchpad button fired, and the unified
//! [`PointerEvent`] stream watchers read from
//! [`InputState::frame_pointer_events`](crate::input::InputState).
//!
//! Wake-gate flags live in
//! [`watches::PointerWake`](crate::input::watch::PointerWake);
//! per-widget hit-test routing lives in
//! [`sense::Sense`](crate::input::sense::Sense). This module is the raw
//! event vocabulary — no routing logic.

use glam::Vec2;
use strum::{EnumCount, EnumIter, IntoEnumIterator};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumCount, EnumIter)]
#[repr(u8)]
pub enum PointerButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

impl PointerButton {
    /// Iterate every variant in declaration order. Wraps
    /// `strum::IntoEnumIterator` so callers don't need to bring the
    /// trait into scope.
    #[inline]
    pub(super) fn all() -> impl Iterator<Item = Self> {
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
/// Sibling of [`KeyboardEvent`](crate::KeyboardEvent) —
/// both live in their own module so the raw-event taxonomy is in one
/// place; [`PointerWake`](crate::PointerWake) and
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
    Down { pos: Vec2, button: PointerButton },
    /// Button released at `pos`. Same gating + routing as `Down`.
    Up { pos: Vec2, button: PointerButton },
    /// Wheel / touchpad scroll at `pos`. `pixels` is pixel-precise
    /// touchpad deltas; `lines` is notched wheel ticks. One or both
    /// may be non-zero per event. Gated on
    /// [`PointerWake::SCROLL`](crate::input::watch::PointerWake::SCROLL).
    Scroll {
        pos: Vec2,
        pixels: Vec2,
        lines: Vec2,
    },
    /// Pinch-zoom factor at `pos`. `factor` is the multiplicative
    /// delta (1.0 = no zoom). Gated on
    /// [`PointerWake::PINCH`](crate::input::watch::PointerWake::PINCH) —
    /// not `SCROLL`, so watching wheel ticks doesn't also wake on pinch.
    Zoom { pos: Vec2, factor: f32 },
    /// Pointer left the surface. No position — by the time this
    /// fires there isn't one. Emitted when any pointer-class
    /// watch is active so watchers can clean up.
    Leave,
}
