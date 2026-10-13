//! [`Watches`]: the wake gates the widgets of one `Ui` declared this record.

use crate::input::keyboard::key_press::KeyPress;
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake};

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
