use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;

/// Payload of [`KeyboardEvent::Down`](crate::KeyboardEvent::Down) — key, modifier snapshot at
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
