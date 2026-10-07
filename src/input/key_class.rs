//! Key classification: the axis input scopes arbitrate on.
//!
//! A scope declares which [`KeyClass`]es it takes via [`KeyFilter`], so a
//! focused editor can own `Ctrl+Z` while `Ctrl+S` reaches the app.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;

/// What kind of key press this is. Exactly one class per press.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyClass {
    /// Printable characters and bare Enter.
    Text,
    /// Ctrl+Z/X/C/V/A, Delete, Backspace.
    Edit,
    /// Arrows, Home and End, under any modifiers.
    Caret,
    /// PageUp and PageDown.
    Page,
    /// Tab and Shift+Tab: focus traversal.
    Focus,
    /// Tab under a command modifier (Ctrl+Tab).
    Cycle,
    /// Escape alone. A field that yields it
    /// ([`crate::TextEdit::escape_falls_through`]) drops the class.
    Escape,
    /// Command chords outside the edit family, and function keys.
    Accel,
}

/// Keys forming an [`KeyClass::Edit`] chord under a command modifier. Kept in
/// step with `EditAction::shortcut` by a test in `widgets::text_edit`.
const EDIT_CHORDS: [char; 5] = ['z', 'x', 'c', 'v', 'a'];

/// Whether `press` is one of [`EDIT_CHORDS`].
///
/// Physical key is only the non-Latin fallback ([`KeyPress::layout_retry`]);
/// a backend leaving it unidentified must not turn edit chords into
/// accelerators. Callers have already excluded presses without a command
/// modifier.
fn is_edit_chord(press: KeyPress) -> bool {
    edit_char(press.key) || press.layout_retry().is_some_and(edit_char)
}

fn edit_char(key: Key) -> bool {
    matches!(key, Key::Char(c) if EDIT_CHORDS.iter().any(|e| e.eq_ignore_ascii_case(&c)))
}

impl KeyClass {
    /// Classify one press.
    ///
    /// Exhaustive over [`Key`] so a new variant must pick a class.
    pub fn of(press: KeyPress) -> Self {
        // Typed text wins whatever the key is called; whether modifiers
        // compose text is the platform's rule (`KeyPress::types_text`).
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
            // A command modifier turns a typed key into a chord; Shift does not.
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
    /// A press goes to the deepest scope whose filter contains its class.
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
    /// `ACCEL` is absent so application chords fall through while typing.
    /// `PAGE`, `FOCUS` and `CYCLE` are absent because a field acts on none
    /// of them.
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
    /// Whether this filter takes `press`'s class. Readers apply it to the
    /// shared stream so a field that yielded a class does not also act on it.
    #[inline]
    pub fn takes_press(self, press: KeyPress) -> bool {
        self.takes(KeyClass::of(press))
    }

    /// [`Self::NONE`] stores "not a scope", fitting the filter in spare
    /// [`crate::scene::node::node_flags::NodeFlags`] bits.
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

    /// `takes_press` classifies the press as [`KeyClass::of`] does.
    #[test]
    fn takes_press_gates_the_stream_on_the_declared_classes() {
        let field = KeyFilter::TEXT_FIELD;
        let typed = KeyPress::with(Key::Char('a'), Modifiers::default());
        let escape = KeyPress::with(Key::Escape, Modifiers::default());

        assert!(field.takes_press(typed), "a field takes text");
        assert!(field.takes_press(escape), "and Escape, to cancel");

        let yields_escape = field.difference(KeyFilter::ESCAPE);
        assert!(!yields_escape.takes_press(escape));
        assert!(yields_escape.takes_press(typed));

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

    /// Navigation class per key, and whether a text field takes it.
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
