//! Key classification — the axis input scopes arbitrate on.
//!
//! A capture that takes *every* key is the wrong granularity for a text
//! field: it swallows the application's accelerators along with the
//! characters. [`KeyClass`] splits a press into one of eight kinds, and a
//! scope declares which kinds it takes via [`KeyFilter`], so a focused
//! editor can own `Ctrl+Z` while `Ctrl+S` walks past it to the app.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;

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
    /// Caret movement, or canvas nudge: the arrows, Home and End, under
    /// any modifiers.
    Caret,
    /// PageUp and PageDown. Apart from [`Self::Caret`] because a field
    /// that moves a caret need not page, and a page key it claimed without
    /// acting on would never reach the scroll view around it.
    Page,
    /// Tab and Shift+Tab: focus traversal. Apart because almost nothing
    /// that edits wants it, and a focused widget that claimed it would cut
    /// the application's traversal off at that widget.
    Focus,
    /// Tab under a command modifier — Ctrl+Tab, Ctrl+Shift+Tab — which
    /// cycles a tab strip or the application's documents. Apart from
    /// [`Self::Focus`] for the reason WPF keeps `ControlTabNavigation`
    /// apart from `TabNavigation`: the widget that cycles on it has no use
    /// for bare Tab, and a text field that takes neither lets both reach
    /// the application.
    Cycle,
    /// Escape alone. Its own class because cancel is hierarchical — the
    /// innermost thing *that can be canceled* should be. Which is not
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
        // A press that typed text *is* text, whatever its key is called:
        // a layout can put a character on a key this vocabulary has no
        // name for, and a dead-key sequence resolves to text the key that
        // carried it never held. Whether the modifiers held composed that
        // text or made a command is the platform's rule —
        // `KeyPress::types_text` — so Option+L is `@` on macOS and Alt+L
        // is a mnemonic on Windows. No named key reaches here with
        // text — what Enter, Tab and Escape produce is a control
        // character, which never enters a `KeyText` — so the match below
        // keeps answering for every key that typed nothing.
        if press.types_text() {
            return Self::Text;
        }
        match press.key {
            Key::Escape => Self::Escape,
            Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End => Self::Caret,
            Key::PageUp | Key::PageDown => Self::Page,
            Key::Tab if press.mods.has_command() => Self::Cycle,
            Key::Tab => Self::Focus,
            Key::Backspace | Key::Delete => Self::Edit,
            // A command modifier is what turns a typed key into a chord:
            // bare `z` is Text, Ctrl+Z is Edit. Shift is not a command —
            // Shift+Z is still typing.
            Key::Char(_) | Key::Enter if !press.mods.has_command() => Self::Text,
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

flag_set! {
    /// The key classes a scope takes while it is on the active path.
    ///
    /// A press walks the active scope path deepest-first and is granted
    /// to the first scope whose filter contains its [`KeyClass`]; scopes
    /// further out never see it.
    pub struct KeyFilter: packed {
        /// Takes [`KeyClass::Text`].
        const TEXT   = 1 << 0;
        /// Takes [`KeyClass::Edit`].
        const EDIT   = 1 << 1;
        /// Takes [`KeyClass::Caret`].
        const CARET  = 1 << 2;
        /// Takes [`KeyClass::Page`].
        const PAGE   = 1 << 3;
        /// Takes [`KeyClass::Focus`].
        const FOCUS  = 1 << 4;
        /// Takes [`KeyClass::Cycle`].
        const CYCLE  = 1 << 5;
        /// Takes [`KeyClass::Escape`].
        const ESCAPE = 1 << 6;
        /// Takes [`KeyClass::Accel`].
        const ACCEL  = 1 << 7;
    }
}

impl KeyFilter {
    /// A focused text field.
    ///
    /// `ACCEL` is **absent**, deliberately: `Ctrl+S` and `Ctrl+R` fall
    /// through to the application while the user is typing. That
    /// omission is the entire reason a scope carries a filter instead of
    /// simply capturing. `PAGE`, `FOCUS` and `CYCLE` are absent because a
    /// field acts on none of them, so Tab reaches the application's focus
    /// traversal while a field holds focus.
    pub const TEXT_FIELD: Self = Self::TEXT
        .union(Self::EDIT)
        .union(Self::CARET)
        .union(Self::ESCAPE);

    /// Whether this filter takes `class`.
    #[inline]
    pub const fn takes(self, class: KeyClass) -> bool {
        self.contains(match class {
            KeyClass::Text => Self::TEXT,
            KeyClass::Edit => Self::EDIT,
            KeyClass::Caret => Self::CARET,
            KeyClass::Page => Self::PAGE,
            KeyClass::Focus => Self::FOCUS,
            KeyClass::Cycle => Self::CYCLE,
            KeyClass::Escape => Self::ESCAPE,
            KeyClass::Accel => Self::ACCEL,
        })
    }

    /// Whether this filter takes `press`'s class.
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
    pub fn takes_press(self, press: KeyPress) -> bool {
        self.takes(KeyClass::of(press))
    }

    /// A scope declaring nothing is not a scope: [`Self::NONE`] is how
    /// "this node is not a scope" is stored, which is what lets the
    /// filter live in spare [`crate::scene::node::node_flags::NodeFlags`]
    /// bits without a separate presence flag.
    #[inline]
    pub(crate) const fn is_scope(self) -> bool {
        !self.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::input::key_class::{KeyClass, KeyFilter};
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_press::KeyPress;
    use crate::input::keyboard::modifiers::Modifiers;

    /// `takes_press` is `takes` over a press: it classifies the press the way
    /// [`KeyClass::of`] does, so one gate serves every reader of the
    /// stream.
    #[test]
    fn takes_press_gates_the_stream_on_the_declared_classes() {
        let field = KeyFilter::TEXT_FIELD;
        let typed = KeyPress::with(Key::Char('a'), Modifiers::default());
        let escape = KeyPress::with(Key::Escape, Modifiers::default());

        assert!(field.takes_press(typed), "a field takes text");
        assert!(field.takes_press(escape), "and Escape, to cancel");

        // Dropping one class drops exactly that class — the shape
        // `TextEdit::escape_falls_through` produces, and the reason its
        // key pass and its context menu apply the same filter.
        let yields_escape = field.difference(KeyFilter::ESCAPE);
        assert!(!yields_escape.takes_press(escape));
        assert!(yields_escape.takes_press(typed));

        // `ACCEL` is out of `TEXT_FIELD`, so an application chord walks
        // past a focused field while the bare key it shares still types.
        let save = KeyPress::with(
            Key::Char('S'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(KeyClass::of(save), KeyClass::Accel);
        assert!(!field.takes_press(save));
        let shifted = KeyPress::with(Key::Char('S'), Modifiers::default());
        assert!(field.takes_press(shifted));
    }

    /// The four navigation classes, and which of them a text field takes.
    /// Tab splits on the command modifier alone: Shift+Tab is still
    /// traversal, and Ctrl, Alt or the raw macOS Control each make it a
    /// cycle.
    #[test]
    fn navigation_keys_split_four_ways() {
        let none = Modifiers::NONE;
        let shift = Modifiers::SHIFT;
        let ctrl = Modifiers::CTRL;
        let ctrl_shift = Modifiers::CTRL_SHIFT;
        let alt = Modifiers::ALT;
        let mac_ctrl = Modifiers {
            mac_ctrl: true,
            ..none
        };
        let cases = [
            (Key::ArrowLeft, none, KeyClass::Caret, true),
            (Key::ArrowRight, shift, KeyClass::Caret, true),
            (Key::ArrowUp, ctrl, KeyClass::Caret, true),
            (Key::ArrowDown, none, KeyClass::Caret, true),
            (Key::Home, ctrl_shift, KeyClass::Caret, true),
            (Key::End, none, KeyClass::Caret, true),
            (Key::PageUp, none, KeyClass::Page, false),
            (Key::PageDown, shift, KeyClass::Page, false),
            (Key::Tab, none, KeyClass::Focus, false),
            (Key::Tab, shift, KeyClass::Focus, false),
            (Key::Tab, ctrl, KeyClass::Cycle, false),
            (Key::Tab, ctrl_shift, KeyClass::Cycle, false),
            (Key::Tab, alt, KeyClass::Cycle, false),
            (Key::Tab, mac_ctrl, KeyClass::Cycle, false),
        ];
        for (key, mods, class, field_takes) in cases {
            let press = KeyPress::with(key, mods);
            assert_eq!(KeyClass::of(press), class, "{key:?} under {mods:?}");
            assert_eq!(
                KeyFilter::TEXT_FIELD.takes_press(press),
                field_takes,
                "{key:?} under {mods:?}",
            );
        }
    }
}
