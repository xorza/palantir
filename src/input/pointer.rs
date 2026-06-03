//! Pointer event taxonomy: the [`PointerButton`] enum identifying
//! which mouse / touchpad button fired, and the unified
//! [`PointerEvent`] stream subscribers read from
//! [`InputState::frame_pointer_events`](crate::input::InputState).
//!
//! Wake-gate flags live in
//! [`subscriptions::PointerSense`](crate::input::subscriptions::PointerSense);
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
    pub fn all() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }

    #[inline]
    pub(crate) fn idx(self) -> usize {
        self as usize
    }
}

/// Unified pointer event stream, populated by
/// [`InputState::on_input`](crate::input::InputState::on_input) when the
/// matching [`PointerSense`](crate::input::subscriptions::PointerSense) flag
/// is set. Each variant is the raw event — "click" is intentionally
/// absent: it's per-widget logic already routed via
/// [`Capture`](crate::input::Capture) →
/// [`ResponseState::clicked`](crate::input::ResponseState::clicked).
///
/// Sibling of [`KeyboardEvent`](crate::input::keyboard::KeyboardEvent) —
/// both live in their own module so the raw-event taxonomy is in one
/// place; wake-gate flags
/// ([`PointerSense`](crate::input::subscriptions::PointerSense),
/// [`KeyboardSense`](crate::input::subscriptions::KeyboardSense)) live in
/// [`subscriptions`](crate::input::subscriptions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved to `pos` (logical pixels). Gated on
    /// [`PointerSense::MOVE`](crate::input::subscriptions::PointerSense::MOVE).
    Move(Vec2),
    /// Button pressed at `pos`. Gated on
    /// [`PointerSense::BUTTONS`](crate::input::subscriptions::PointerSense::BUTTONS).
    /// Hit-test + capture routing happens independently; subscribers
    /// see every press regardless of where it landed.
    Down { pos: Vec2, button: PointerButton },
    /// Button released at `pos`. Same gating + routing as `Down`.
    Up { pos: Vec2, button: PointerButton },
    /// Wheel / touchpad scroll at `pos`. `pixels` is pixel-precise
    /// touchpad deltas; `lines` is notched wheel ticks. One or both
    /// may be non-zero per event. Gated on
    /// [`PointerSense::SCROLL`](crate::input::subscriptions::PointerSense::SCROLL).
    Scroll {
        pos: Vec2,
        pixels: Vec2,
        lines: Vec2,
    },
    /// Pinch-zoom factor at `pos`. `factor` is the multiplicative
    /// delta (1.0 = no zoom). Gated on
    /// [`PointerSense::SCROLL`](crate::input::subscriptions::PointerSense::SCROLL).
    Zoom { pos: Vec2, factor: f32 },
    /// Pointer left the surface. No position — by the time this
    /// fires there isn't one. Emitted when any pointer-class
    /// subscription is active so subscribers can clean up.
    Leave,
}
