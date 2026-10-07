//! Off-target wake gates, re-asserted by widgets each frame they are active
//! and cleared before each record. The set persists across silent frames, so
//! a dormant popup keeps `BUTTONS` for the next outside click. Events are
//! queued in [`InputState`](crate::input::input_state::InputState) only while
//! a relevant watch is active.

use crate::input::keyboard::key_press::KeyPress;
use crate::input::shortcut::Shortcut;

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
        /// Wakes on any [`KeyPress`] regardless of focus.
        const KEY = 1 << 0;
        /// Wakes on `ModifiersChanged`.
        const MODIFIER = 1 << 1;
    }
}

/// Per-`Ui` wake-gate registry, re-declared by widgets during record.
#[derive(Debug, Default)]
pub(super) struct Watches {
    pub(super) pointer_mask: PointerWake,
    pub(super) keyboard_mask: KeyboardWake,
    pub(super) keys: Vec<Shortcut>,
}

impl Watches {
    /// Pushed unconditionally, duplicates included: a `contains` would cost O(n²) per poll.
    pub(super) fn watch_key(&mut self, sc: Shortcut) {
        self.keys.push(sc);
    }

    /// Whether a key press would wake any chord watcher; takes the whole [`KeyPress`] for [`Shortcut::matches`]'s layout fallback.
    pub(super) fn matches_press(&self, kp: KeyPress) -> bool {
        self.keys.iter().any(|s| s.matches(kp))
    }

    /// Capacity-retained clear, run before every full record.
    pub(super) fn clear(&mut self) {
        self.pointer_mask = PointerWake::NONE;
        self.keyboard_mask = KeyboardWake::NONE;
        self.keys.clear();
    }
}
