use crate::primitives::span::Span;
use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Arena-backed text handle. Every value is a span into storage owned by
/// [`crate::Ui`]; construct one with [`crate::Ui::intern`] or
/// [`crate::Ui::fmt`].
///
/// Retaining a handle keeps its exact source bytes alive. Lowering is zero-copy
/// when the handle belongs to the active record store and copies into that
/// store when it came from an earlier pass or another window.
#[derive(Clone, Debug)]
pub struct InternedStr {
    pub(crate) span: Span,
    pub(crate) arena: Rc<TextArena>,
}

/// Transient text accepted by widget builders. Borrowed and owned inputs are
/// copied into the active [`crate::Ui`] text arena when the widget is shown;
/// an [`InternedStr`] is already there and passes through unchanged.
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

/// Retained bytes behind frame-authored [`InternedStr`] values. The active
/// record store reuses its arena while no handle has escaped; an escaped
/// handle keeps this allocation and its exact bytes alive.
#[derive(Debug, Default)]
pub(crate) struct TextArena {
    pub(crate) bytes: RefCell<String>,
}

/// Text stored on a [`ShapeRecord`](crate::forest::shapes::record::ShapeRecord).
/// Its span always addresses the active record store because lowering rebases
/// handles from any other arena before constructing this value.
#[derive(Clone, Debug)]
pub(crate) struct RecordedText {
    span: Span,
    hash: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedText<'a> {
    pub(crate) text: &'a str,
    pub(crate) hash: u64,
}

impl InternedStr {
    pub(crate) fn arena_backed(span: Span, arena: Rc<TextArena>) -> Self {
        Self { span, arena }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.span.len == 0
    }

    /// Borrow this handle's string slice from its owning arena.
    pub fn borrow_str(&self) -> Ref<'_, str> {
        Ref::map(self.arena.bytes.borrow(), |bytes| &bytes[self.span.range()])
    }
}

impl Default for TextInput<'_> {
    fn default() -> Self {
        Self::Borrowed("")
    }
}

impl<'a> From<&'a str> for TextInput<'a> {
    fn from(text: &'a str) -> Self {
        Self::Borrowed(text)
    }
}

impl<'a> From<&'a String> for TextInput<'a> {
    fn from(text: &'a String) -> Self {
        Self::Borrowed(text)
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

impl RecordedText {
    pub(crate) fn new(span: Span, hash: u64) -> Self {
        Self { span, hash }
    }

    /// Resolve the paired recorded bytes and content hash. The span is
    /// guaranteed to target `text_bytes` by `RecordStore::record_text`.
    #[inline]
    pub(crate) fn resolve<'a>(&'a self, text_bytes: &'a str) -> ResolvedText<'a> {
        let text =
            &text_bytes[self.span.start as usize..(self.span.start + self.span.len) as usize];
        ResolvedText {
            text,
            hash: self.hash,
        }
    }
}

impl Hash for RecordedText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::interned_str::{InternedStr, TextArena, TextInput};
    use crate::primitives::span::Span;
    use std::rc::Rc;

    #[test]
    fn text_input_empty_tracks_every_storage_variant() {
        assert!(TextInput::default().is_empty());
        assert!(!TextInput::Borrowed("x").is_empty());
        assert!(TextInput::Owned(String::new()).is_empty());
        assert!(!TextInput::Owned("x".to_owned()).is_empty());

        let arena = Rc::new(TextArena::default());
        arena.bytes.borrow_mut().push('x');
        assert!(
            TextInput::Interned(InternedStr::arena_backed(Span::new(0, 0), arena.clone()))
                .is_empty()
        );
        assert!(!TextInput::Interned(InternedStr::arena_backed(Span::new(0, 1), arena)).is_empty());
    }
}
