//! Why a character names no [`Key`](crate::Key).

use std::error;
use std::fmt::{self, Display, Formatter};

/// A control character, which [`Key::Char`](crate::Key::Char) never holds: a key press carries no
/// such text, and its key has a name, as [`Key::Enter`](crate::Key::Enter), or is
/// [`Key::Other`](crate::Key::Other).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCharError {
    /// The character refused.
    pub found: char,
}

impl Display for ControlCharError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is a control character, which no key types; name the key instead",
            self.found
        )
    }
}

impl error::Error for ControlCharError {}
