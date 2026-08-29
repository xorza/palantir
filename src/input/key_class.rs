//! Key classification — the axis input scopes arbitrate on.
//!
//! A capture that takes *every* key is the wrong granularity for a text
//! field: it swallows the application's accelerators along with the
//! characters. [`KeyClass`] splits a press into one of five kinds, and a
//! scope declares which kinds it takes via [`KeyFilter`], so a focused
//! editor can own `Ctrl+Z` while `Ctrl+S` walks past it to the app.

use bitflags::bitflags;

use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::keyboard_event::KeyboardEvent;

/// What kind of thing a key press *is*. Exactly one class per press.
///
/// The split exists so a focused text field can take the keys it edits
/// with without also swallowing the application's accelerators — which
/// is the whole difference between a scope filter and an exclusive
/// capture.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyClass {
    /// Printable characters and bare Enter. Only a text field wants these.
    Text,
    /// The clipboard/undo family plus the destructive edit keys:
    /// Ctrl+Z/X/C/V/A, Delete, Backspace. The contested class — a text
    /// field and a canvas both want it, and deciding between them is what
    /// scopes exist for.
    Edit,
    /// Caret movement, or canvas nudge: arrows, Home/End, PgUp/PgDn, Tab.
    Motion,
    /// Escape alone. Its own class because cancel is hierarchical — the
    /// innermost thing *that can be cancelled* should be. Which is not
    /// always the innermost scope: a field that filters its container
    /// rather than editing a value has nothing of its own to cancel, and
    /// drops the class so the container gets it
    /// ([`crate::TextEdit::escape_falls_through`]).
    Escape,
    /// Everything else: command chords outside the edit family, and the
    /// function keys. Ctrl+S, Ctrl+R, F12. Commands, never editing.
    Accel,
}

/// The keys that form an [`KeyClass::Edit`] chord under a command
/// modifier.
///
/// Kept in step with `EditAction::shortcut` by
/// `widgets::text_edit::tests::every_edit_action_chord_is_edit_class`: a
/// seventh edit action that forgets to extend this list fails that test
/// rather than silently becoming an accelerator the app steals.
const EDIT_CHORDS: [char; 5] = ['z', 'x', 'c', 'v', 'a'];

/// Whether `press` is one of [`EDIT_CHORDS`].
///
/// Logical key first, physical only as the non-Latin fallback — the one
/// [`KeyPress::layout_retry`] states, which is also what
/// [`crate::Shortcut::matches`] retries against. Keying off `physical`
/// alone looks equivalent and is not: a backend that leaves it
/// unidentified would turn every edit chord into an accelerator and hand
/// a focused editor's undo to the app.
///
/// No modifier gate of its own: the arm above this one in
/// [`KeyClass::of`] already claimed every press without a command
/// modifier, so a press that reaches here holds one.
///
/// Case-insensitive, like `Shortcut`'s own `Char` comparison — a logical
/// key arrives post-shift, so `Ctrl+Shift+Z` is `Char('Z')`.
fn is_edit_chord(press: KeyPress) -> bool {
    edit_char(press.key) || press.layout_retry().is_some_and(edit_char)
}

fn edit_char(key: Key) -> bool {
    matches!(key, Key::Char(c) if EDIT_CHORDS.iter().any(|e| e.eq_ignore_ascii_case(&c)))
}

impl KeyClass {
    /// Classify one press.
    ///
    /// Exhaustive over [`Key`] on purpose: a new key variant fails to
    /// compile until it declares which class it belongs to, rather than
    /// falling into a catch-all and quietly becoming an accelerator.
    pub fn of(press: KeyPress) -> Self {
        match press.key {
            Key::Escape => Self::Escape,
            Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Tab => Self::Motion,
            Key::Backspace | Key::Delete => Self::Edit,
            // A command modifier is what turns a typed key into a chord:
            // bare `z` is Text, Ctrl+Z is Edit. Shift is not a command —
            // Shift+Z is still typing.
            Key::Char(_) | Key::Enter if !press.mods.any_command() => Self::Text,
            Key::Char(_) if is_edit_chord(press) => Self::Edit,
            Key::Char(_) | Key::Enter => Self::Accel,
            Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12 => Self::Accel,
            Key::Other => Self::Accel,
        }
    }
}

bitflags! {
    /// The key classes a scope takes while it is on the active path.
    ///
    /// A press walks the active scope path deepest-first and is granted
    /// to the first scope whose filter contains its [`KeyClass`]; scopes
    /// further out never see it.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct KeyFilter: u8 {
        const TEXT   = 1 << 0;
        const EDIT   = 1 << 1;
        const MOTION = 1 << 2;
        const ESCAPE = 1 << 3;
        const ACCEL  = 1 << 4;
    }
}

impl KeyFilter {
    /// Every class — an overlay that owns the keyboard outright. What
    /// `Popup` and `Modal` declare: a whole-stream claim, expressed as a
    /// filter rather than as a separate capture mechanism.
    pub const ALL: Self = Self::all();

    /// A focused text field.
    ///
    /// `ACCEL` is **absent**, deliberately: `Ctrl+S` and `Ctrl+R` fall
    /// through to the application while the user is typing. That
    /// omission is the entire reason a scope carries a filter instead of
    /// simply capturing.
    pub const TEXT_FIELD: Self = Self::TEXT
        .union(Self::EDIT)
        .union(Self::MOTION)
        .union(Self::ESCAPE);

    /// Whether this filter takes `class`.
    #[inline]
    pub fn takes(self, class: KeyClass) -> bool {
        self.contains(match class {
            KeyClass::Text => Self::TEXT,
            KeyClass::Edit => Self::EDIT,
            KeyClass::Motion => Self::MOTION,
            KeyClass::Escape => Self::ESCAPE,
            KeyClass::Accel => Self::ACCEL,
        })
    }

    /// `event` back when this filter takes its class, `None` otherwise.
    ///
    /// **The gate a reader applies to the stream it drains**, not only to
    /// the scope it declares. The stream is the whole layer's, so a field
    /// that told every other reader it does not take a class — a
    /// [`TextEdit`](crate::TextEdit) with `escape_falls_through` — would
    /// otherwise go on acting on it anyway, while the container the class
    /// was yielded to acts on it too. One press, handled twice, which is
    /// the exact double dispatch scopes exist to prevent.
    ///
    /// One place rather than one per drain: a field's key pass and its
    /// context menu read the same stream through the same filter.
    #[inline]
    pub fn accepts(self, event: KeyboardEvent) -> Option<KeyboardEvent> {
        let class = match event {
            KeyboardEvent::Down(keypress) => KeyClass::of(keypress),
            KeyboardEvent::Text(_) => KeyClass::Text,
        };
        self.takes(class).then_some(event)
    }

    /// A scope declaring nothing is not a scope: [`Self::empty`] is how
    /// "this node is not a scope" is stored, which is what lets the
    /// filter live in spare [`crate::scene::node::node_flags::NodeFlags`]
    /// bits without a separate presence flag.
    #[inline]
    pub(crate) fn is_scope(self) -> bool {
        !self.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::input::key_class::{KeyClass, KeyFilter};
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_press::KeyPress;
    use crate::input::keyboard::keyboard_event::KeyboardEvent;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::input::keyboard::text_chunk::TextChunk;

    fn press(key: Key, mods: Modifiers) -> KeyPress {
        KeyPress {
            key,
            mods,
            repeat: false,
            physical: key,
        }
    }

    /// `accepts` is `takes` over a whole event: it classifies a press the
    /// way [`KeyClass::of`] does and reads a text commit as
    /// [`KeyClass::Text`], so one gate covers both arms of the stream.
    #[test]
    fn accepts_gates_both_arms_of_the_stream_on_the_declared_classes() {
        let field = KeyFilter::TEXT_FIELD;
        let commit = KeyboardEvent::Text(TextChunk::new("a").expect("a one-char chunk fits"));
        let escape = KeyboardEvent::Down(press(Key::Escape, Modifiers::default()));

        assert_eq!(field.accepts(commit), Some(commit), "a field takes text");
        assert_eq!(field.accepts(escape), Some(escape), "and Escape, to cancel");

        // Dropping one class drops exactly that class — the shape
        // `TextEdit::escape_falls_through` produces, and the reason its
        // key pass and its context menu apply the same filter.
        let yields_escape = field.difference(KeyFilter::ESCAPE);
        assert_eq!(yields_escape.accepts(escape), None);
        assert_eq!(yields_escape.accepts(commit), Some(commit));

        // `ACCEL` is out of `TEXT_FIELD`, so an application chord walks
        // past a focused field while the bare key it shares still types.
        let save = press(
            Key::Char('S'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(KeyClass::of(save), KeyClass::Accel);
        assert_eq!(field.accepts(KeyboardEvent::Down(save)), None);
        let typed = KeyboardEvent::Down(press(Key::Char('S'), Modifiers::default()));
        assert_eq!(field.accepts(typed), Some(typed));
    }
}
