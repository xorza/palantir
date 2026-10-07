//! One key-down as the input queue carries it: key, modifiers, repeat.

use crate::common::platform::PLATFORM;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;

/// One entry of the per-frame keyboard queue. `mods` is captured when the event was pushed: key and modifier events arrive interleaved, so snapshotting at drain time mis-attributes mods on rapid chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    /// The **logical** key, after layout and Shift (Shift+'a' arrives as `Char('A')`); for layout-independent matching use [`Self::physical`].
    pub key: Key,
    /// Modifiers when the event was pushed, not drained.
    pub mods: Modifiers,
    /// `true` for OS key-repeat re-emissions; some commands (focus-cycle on Tab) fire only on `!repeat`.
    pub repeat: bool,
    /// The key at this physical position, independent of layout (`Char('z')` for the physical Z key whatever the layout maps it to); lets [`crate::Shortcut`] match a chord whose logical [`key`](Self::key) is non-Latin (Cyrillic `'я'`, see [`crate::Shortcut::matches`]).
    pub physical: Key,
    /// What this press produced to type, where `key` is what it produced to match, see [`KeyText`]; empty for a named key, a chord, or a pending dead key.
    pub text: KeyText,
}

impl KeyPress {
    /// A press that typed `text` and is no key (an IME commit); no modifiers.
    pub(crate) const fn typed(text: KeyText) -> Self {
        Self {
            key: Key::Other,
            mods: Modifiers::NONE,
            repeat: false,
            physical: Key::Other,
            text,
        }
    }

    /// The layout-independent key to retry a chord against when the logical key is not Latin. Dvorak and AZERTY already produce ASCII, so retrying there would fire the wrong chord. Shared by [`Shortcut::matches`](crate::Shortcut::matches) and `KeyClass`'s edit chords.
    pub(crate) fn layout_retry(self) -> Option<Key> {
        matches!(self.key, Key::Char(c) if !c.is_ascii()).then_some(self.physical)
    }

    /// Whether this press typed its [`Self::text`], the one rule the key classifier and text fields share.
    pub(crate) fn types_text(self) -> bool {
        !self.text.is_empty() && self.mods.compose_text(PLATFORM)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_press::KeyPress;
    use crate::input::keyboard::key_text::KeyText;
    use crate::input::keyboard::modifiers::Modifiers;

    impl KeyPress {
        /// A first press of `key` under `mods`, typing what the key types on a plain layout; `physical` is [`Key::Other`] (only a non-ASCII `Char` under a command modifier consults it).
        pub(crate) fn with(key: Key, mods: Modifiers) -> Self {
            Self {
                key,
                mods,
                repeat: false,
                physical: Key::Other,
                text: KeyText::of_key(key),
            }
        }
    }
}
