//! Off-target wake gates, re-asserted by widgets each frame they are active
//! and cleared before each record. The set persists across silent frames, so
//! a dormant popup keeps `BUTTONS` for the next outside click. Events are
//! queued in [`Input`](crate::input::Input) only while
//! a relevant watch is active.

flag_set! {
    /// Wake-gate categories, granular so a click watcher doesn't wake on every move.
    pub struct PointerWake: u8 {
        /// Wakes on [`PointerEvent::Pressed`](crate::PointerEvent::Pressed) and [`PointerEvent::Released`](crate::PointerEvent::Released).
        const BUTTONS = 1 << 0;
        /// Wakes on [`PointerEvent::Moved`](crate::PointerEvent::Moved); expensive in event count.
        const MOVE = 1 << 1;
        /// Wakes on [`PointerEvent::Scroll`](crate::PointerEvent::Scroll).
        const SCROLL = 1 << 2;
        /// Wakes on [`PointerEvent::Pinch`](crate::PointerEvent::Pinch), separate from `SCROLL` like [`Sense::PINCH`](crate::Sense::PINCH).
        const PINCH = 1 << 3;
    }
}

flag_set! {
    /// Keyboard wake-gate categories for **off-focus** consumers; a focused widget always wakes on `KeyDown`.
    pub struct KeyboardWake: u8 {
        /// Wakes on any [`KeyPress`](crate::primitives::input::keyboard::key_press::KeyPress) regardless of focus.
        const KEY = 1 << 0;
        /// Wakes on `ModifiersChanged`.
        const MODIFIER = 1 << 1;
    }
}
