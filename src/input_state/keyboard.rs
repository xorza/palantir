//! [`Keyboard`]: the keys down, this frame's presses and releases, and the modifiers.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;

/// One press held down: the logical key it reported and the physical key it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HeldKey {
    pub(super) key: Key,
    pub(super) physical: Key,
}

/// What the keyboard leaves behind between events, and this frame's edges.
#[derive(Debug, Default)]
pub(super) struct Keyboard {
    /// This frame's presses, in arrival order.
    pub(super) presses: Vec<KeyPress>,
    /// The keys down now, as the logical and physical keys of each press: two physical keys can
    /// share one logical key, as both Enter keys do, so each holds its own entry. A second press
    /// of one pair is a lost release and holds nothing more, so a host that drops releases
    /// cannot grow the list past its keyboard. Persists across frames.
    pub(super) held: Vec<HeldKey>,
    /// This frame's releases, a focus loss's included.
    pub(super) released: Vec<Key>,
    /// Persists across frames; updated only on `ModifiersChanged` and a focus loss.
    pub(super) modifiers: Modifiers,
}

impl Keyboard {
    /// Note `held` down, unless the press repeats or the pair is down already.
    pub(super) fn hold(&mut self, held: HeldKey, repeat: bool) {
        if !repeat && !self.held.contains(&held) {
            self.held.push(held);
        }
    }

    /// Release the press of `key`; `false` when none holds it.
    pub(super) fn release(&mut self, key: Key) -> bool {
        let Some(idx) = self.held.iter().position(|held| held.key == key) else {
            return false;
        };
        self.held.swap_remove(idx);
        self.released.push(key);
        true
    }

    /// The surface lost the keyboard: no modifier is down, and every held key ends, with its
    /// release edge, as Bevy's `ButtonInput` reports it.
    pub(super) fn lose_surface(&mut self) {
        self.modifiers = Modifiers::NONE;
        self.released
            .extend(self.held.drain(..).map(|held| held.key));
    }

    /// Type `text` in place among the presses as if each character had its own key, a piece of
    /// a key's text at a time; no modifiers.
    pub(super) fn type_text(&mut self, text: &str) {
        let mut piece = KeyText::EMPTY;
        for c in text.chars() {
            if !piece.push(c) {
                self.presses.push(KeyPress::typed(piece));
                piece = KeyText::EMPTY;
                piece.push(c);
            }
        }
        if !piece.is_empty() {
            self.presses.push(KeyPress::typed(piece));
        }
    }

    /// Whether `key` is down now, whatever the modifiers.
    pub(super) fn is_held(&self, key: Key) -> bool {
        self.held.iter().any(|held| held.key.same_key(key))
    }

    /// Whether `key` came up this frame.
    pub(super) fn was_released(&self, key: Key) -> bool {
        self.released.iter().any(|released| released.same_key(key))
    }

    /// Forget this frame's edges.
    pub(super) fn end_frame(&mut self) {
        self.presses.clear();
        self.released.clear();
    }
}
