//! [`Watches`]: the wake gates the widgets of one `Ui` declared this record.

use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};

/// Per-`Ui` wake-gate registry, re-declared by widgets during record. Bounded by the reads of one
/// frame's two passes, its capacity kept.
#[derive(Debug, Default)]
pub(super) struct Watches {
    pub(super) pointer_mask: PointerWake,
    pub(super) keyboard_mask: KeyboardWake,
    pub(super) shortcuts: Vec<Shortcut>,
    /// Keys watched whatever the modifiers: their presses and their releases wake.
    pub(super) keys: Vec<Key>,
}

impl Watches {
    /// Pushed unconditionally, duplicates included: a `contains` would cost O(n²) per poll.
    pub(super) fn watch_shortcut(&mut self, sc: Shortcut) {
        self.shortcuts.push(sc);
    }

    /// Pushed unconditionally, as [`Self::watch_shortcut`].
    pub(super) fn watch_key(&mut self, key: Key) {
        self.keys.push(key);
    }

    /// Whether a key press would wake any watcher; takes the whole [`KeyPress`] for [`Shortcut::matches`]'s layout fallback.
    pub(super) fn matches_press(&self, kp: KeyPress) -> bool {
        self.keys.iter().any(|key| key.same_key(kp.key))
            || self.shortcuts.iter().any(|s| s.matches(kp))
    }

    /// Whether the release of `key` would wake a watcher.
    pub(super) fn watches_key(&self, key: Key) -> bool {
        self.keys.iter().any(|watched| watched.same_key(key))
    }

    /// Capacity-retained clear, run as a frame's first pass opens.
    pub(super) fn clear(&mut self) {
        self.pointer_mask = PointerWake::NONE;
        self.keyboard_mask = KeyboardWake::NONE;
        self.shortcuts.clear();
        self.keys.clear();
    }
}
