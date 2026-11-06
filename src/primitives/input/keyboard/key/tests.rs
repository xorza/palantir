use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key::error::ControlCharError;

/// A printing character is the key that types it, a space and a non-ASCII letter included; a
/// control character is refused, its key a named one or none.
#[test]
fn a_printing_character_is_a_key_and_a_control_one_is_refused() {
    for c in ['Z', 'z', ' ', '+', 'ζ'] {
        assert_eq!(Key::try_from(c), Ok(Key::Char(c)), "{c:?}");
    }
    for c in ['\n', '\t', '\u{1b}', '\u{7f}', '\0'] {
        assert_eq!(
            Key::try_from(c),
            Err(ControlCharError { found: c }),
            "{c:?}"
        );
    }
}
