//! The text a widget builder accepts, in whichever form the caller has.

use crate::primitives::interned_str::InternedStr;
use std::borrow::Cow;

/// Transient text accepted by widget builders. Borrowed and owned inputs
/// are copied into the active [`crate::Ui`] text arena when the widget is
/// shown; an [`InternedStr`] is already there and passes through
/// unchanged, provided it belongs to the pass doing the showing.
#[derive(Debug)]
pub enum TextInput<'a> {
    Borrowed(&'a str),
    Owned(String),
    Interned(InternedStr),
}

impl TextInput<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Borrowed(text) => text.is_empty(),
            Self::Owned(text) => text.is_empty(),
            Self::Interned(text) => text.is_empty(),
        }
    }
}

impl Default for TextInput<'_> {
    fn default() -> Self {
        Self::Borrowed("")
    }
}

impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for TextInput<'a> {
    fn from(text: &'a T) -> Self {
        Self::Borrowed(text.as_ref())
    }
}

impl<'a> From<String> for TextInput<'a> {
    fn from(text: String) -> Self {
        Self::Owned(text)
    }
}

impl<'a> From<InternedStr> for TextInput<'a> {
    fn from(text: InternedStr) -> Self {
        Self::Interned(text)
    }
}

impl<'a> From<Cow<'a, str>> for TextInput<'a> {
    fn from(text: Cow<'a, str>) -> Self {
        match text {
            Cow::Borrowed(text) => Self::Borrowed(text),
            Cow::Owned(text) => Self::Owned(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::interned_str::InternedStr;
    use crate::primitives::span::Span;
    use crate::primitives::text_epoch::TextEpoch;
    use crate::primitives::text_input::TextInput;
    use std::borrow::Cow;

    #[test]
    fn text_input_empty_tracks_every_storage_variant() {
        assert!(TextInput::default().is_empty());
        assert!(!TextInput::Borrowed("x").is_empty());
        assert!(TextInput::Owned(String::new()).is_empty());
        assert!(!TextInput::Owned("x".to_owned()).is_empty());

        let epoch = TextEpoch::next();
        assert!(TextInput::Interned(InternedStr::new(Span::new(0, 0), epoch)).is_empty());
        assert!(!TextInput::Interned(InternedStr::new(Span::new(0, 1), epoch)).is_empty());

        let nested_borrow = "nested";
        let TextInput::Borrowed(text) = TextInput::from(&nested_borrow) else {
            panic!("nested string borrow must stay borrowed");
        };
        assert_eq!(text, "nested");

        // Every epoch is distinct, which is what separates one pass's
        // handles from the next's and one window's from another's.
        assert_ne!(TextEpoch::next(), TextEpoch::next());

        let cow = Cow::Borrowed("cow");
        let TextInput::Borrowed(text) = TextInput::from(&cow) else {
            panic!("borrowed Cow must stay borrowed");
        };
        assert_eq!(text, "cow");
    }
}
