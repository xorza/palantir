//! [`Watches`]: the wake gates the widgets of one `Ui` declared this record.

use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};

/// How far [`Watches`] had come when the input stage closed: each pass goes back to this, so its
/// watches outlive the pass's reset. Refused: a second `Watches` to copy from, which holds each
/// watch twice and copies them per pass.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WatchMark {
    pointer_mask: PointerWake,
    keyboard_mask: KeyboardWake,
    shortcuts: usize,
    keys: usize,
}

/// Per-`Ui` wake-gate registry, re-declared by widgets during record. Bounded by the reads one
/// record makes, as each record starts from what the input stage declared, its capacity kept.
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

    /// What these watches hold now, for [`Self::rewind`].
    pub(super) const fn mark(&self) -> WatchMark {
        WatchMark {
            pointer_mask: self.pointer_mask,
            keyboard_mask: self.keyboard_mask,
            shortcuts: self.shortcuts.len(),
            keys: self.keys.len(),
        }
    }

    /// Drop every watch made since `mark`. Capacity-retained, run before each record pass.
    pub(super) fn rewind(&mut self, mark: WatchMark) {
        self.pointer_mask = mark.pointer_mask;
        self.keyboard_mask = mark.keyboard_mask;
        self.shortcuts.truncate(mark.shortcuts);
        self.keys.truncate(mark.keys);
    }

    /// Capacity-retained clear, run as a recorded frame opens.
    pub(super) fn clear(&mut self) {
        self.pointer_mask = PointerWake::NONE;
        self.keyboard_mask = KeyboardWake::NONE;
        self.shortcuts.clear();
        self.keys.clear();
    }
}
