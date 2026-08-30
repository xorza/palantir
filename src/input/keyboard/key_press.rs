//! One key-down as the input queue carries it: which key, which
//! modifiers, and whether it repeated.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;

/// One entry of the per-frame keyboard queue — key, modifier snapshot at
/// push time, repeat flag. Modifiers and key events arrive
/// interleaved over the wire, so snapshotting at drain time would
/// mis-attribute mods on rapid chord input — `mods` is captured
/// when the event was pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    /// The **logical** key, after the active layout and Shift have been
    /// applied — Shift+'a' arrives as `Char('A')`. For layout-independent
    /// chord matching use [`Self::physical`].
    pub key: Key,
    /// Modifier state captured when the event was pushed, not when it was
    /// drained — rapid chord input would otherwise mis-attribute mods.
    pub mods: Modifiers,
    /// `true` for OS-level key-repeat re-emissions; `false` for the
    /// initial press. Editors typically treat both the same; some
    /// commands (e.g. focus-cycle on Tab) only fire on `!repeat`.
    pub repeat: bool,
    /// The key at this physical position, identified **independent of the
    /// active layout** — `Char('z')` for the physical Z key whatever the layout
    /// maps it to, `Enter` / `ArrowLeft` / … for named keys, `Other` for an
    /// unidentified position. Lets [`crate::Shortcut`] recover a command chord
    /// whose logical [`key`](Self::key) arrived as a non-Latin character
    /// (Cyrillic `'я'` for the physical Z on a Russian layout — see
    /// [`crate::Shortcut::matches`]).
    pub physical: Key,
}

impl KeyPress {
    /// The layout-independent key to retry a chord against, when the
    /// logical one is not Latin.
    ///
    /// A non-Latin layout still puts `Z` where a US keyboard does, so a
    /// chord declared on `Z` has to be matched against
    /// [`Self::physical`] — but only there. Dvorak and AZERTY already
    /// produce ASCII letters, in their own intended positions, and
    /// retrying would fire the wrong chord.
    ///
    /// One rule, read by [`Shortcut::matches`](crate::Shortcut::matches)
    /// and by `KeyClass`'s edit chords, so the two cannot disagree about
    /// what `Ctrl+Z` is.
    pub(crate) fn layout_retry(self) -> Option<Key> {
        matches!(self.key, Key::Char(c) if !c.is_ascii()).then_some(self.physical)
    }
}
