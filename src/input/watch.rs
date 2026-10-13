//! Off-target wake gates, re-asserted by widgets each frame they are active
//! and cleared before each record. The set persists across silent frames, so
//! a dormant popup keeps `BUTTONS` for the next outside click. Events are
//! queued in [`InputState`](crate::input_state::InputState) only while
//! a relevant watch is active.

flag_set! {
    /// Wake-gate categories, granular so a click watcher doesn't wake on every move.
    pub struct PointerWake {
        /// Wakes on [`PointerEvent::Down`](crate::PointerEvent::Down) and [`PointerEvent::Up`](crate::PointerEvent::Up).
        const BUTTONS = 1 << 0;
        /// Wakes on [`PointerEvent::Move`](crate::PointerEvent::Move); expensive in event count.
        const MOVE = 1 << 1;
        /// Wakes on [`PointerEvent::Scroll`](crate::PointerEvent::Scroll).
        const SCROLL = 1 << 2;
        /// Wakes on [`PointerEvent::Zoom`](crate::PointerEvent::Zoom), separate from `SCROLL` like [`Sense::PINCH`](crate::Sense::PINCH).
        const PINCH = 1 << 3;
    }
}

flag_set! {
    /// Keyboard wake-gate categories for **off-focus** consumers; a focused widget always wakes on `KeyDown`.
    pub struct KeyboardWake {
        /// Wakes on any [`KeyPress`](crate::input::keyboard::key_press::KeyPress) regardless of focus.
        const KEY = 1 << 0;
        /// Wakes on `ModifiersChanged`.
        const MODIFIER = 1 << 1;
    }
}
