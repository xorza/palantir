//! Why text cannot make a [`Shortcut`](crate::Shortcut).

use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`Shortcut`](crate::Shortcut)'s `FromStr` refused its text, which comes from an agent, a
/// script or a settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseShortcutError {
    /// No key: the text is empty, or ends in a modifier.
    NoKey,
    /// A modifier that is not `ctrl`, `cmd`, `shift`, `alt`, `meta`, `super` or `win`.
    Modifier,
    /// A key that is neither a named key nor one character.
    Key,
}

impl Display for ParseShortcutError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoKey => "expected modifiers and one key joined by '+', as ctrl+shift+z",
            Self::Modifier => "expected the modifiers ctrl, cmd, shift, alt, meta, super or win",
            Self::Key => {
                "expected a named key (enter, escape, tab, backspace, delete, the arrows, home, \
                 end, pageup, pagedown, space, f1 to f12) or one character"
            }
        })
    }
}

impl error::Error for ParseShortcutError {}
