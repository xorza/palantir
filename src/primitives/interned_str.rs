use crate::primitives::span::Span;
use std::borrow::Cow;
use std::cell::Ref;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Text handle valid for **the record pass that minted it**, in the
/// window that minted it. A span into that pass's arena; construct one
/// with [`crate::Ui::intern`] or [`crate::Ui::fmt`], and lower it in the
/// same pass.
///
/// Lowering is zero-copy: the bytes are already where the record store
/// wants them, so recording is a span plus a hash.
///
/// **Intern per frame, per window.** Holding a handle past its pass —
/// into the next frame, into the second pass of a double-layout frame,
/// or into another window's — is a contract violation, and
/// [`crate::Ui`] panics rather than resolving it against whatever text
/// now occupies those offsets. Handles are [`Copy`] and carry no
/// ownership, so nothing keeps the bytes alive on their behalf.
///
/// Persistent application text belongs in its own `String`, passed to
/// widgets by reference; interning it each frame costs one `memcpy` into
/// the arena, which is what the borrowed path pays anyway.
#[derive(Clone, Copy, Debug)]
pub struct InternedStr {
    pub(crate) span: Span,
    pub(crate) epoch: TextEpoch,
}

/// Identity of one record pass's text arena.
///
/// Drawn from a process-wide counter, so it separates passes *and*
/// windows with one comparison — a handle from another window can no
/// more match than one from another frame, and neither needs a second
/// field to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEpoch(u64);

impl TextEpoch {
    /// The next unused epoch. Taken once per record-pass reset, so the
    /// atomic is nowhere near a hot path.
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

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

/// Borrow of the complete record-pass text arena. Recorded text spans resolve
/// against this value while its borrow guard keeps the arena immutable.
#[derive(Debug)]
pub(crate) struct InternedText<'a> {
    pub(crate) bytes: Ref<'a, str>,
}

/// Compact reference to source bytes in the active record store.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextSource {
    pub(crate) span: Span,
}

/// Text stored on a [`ShapeRecord`](crate::scene::shapes::record::ShapeRecord).
/// Its span always addresses the active record store because lowering rebases
/// handles from any other arena before constructing this value.
#[derive(Clone, Debug)]
pub(crate) struct RecordedText {
    pub(crate) source: TextSource,
    /// `hash_str` of the recorded bytes, computed once at record time.
    /// Downstream consumers (scene identity, [`crate::text::key::TextShapeKey`])
    /// reuse it instead of rescanning the text.
    pub(crate) hash: u64,
}

impl InternedStr {
    pub(crate) fn new(span: Span, epoch: TextEpoch) -> Self {
        Self { span, epoch }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.span.len == 0
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

impl RecordedText {
    pub(crate) fn new(span: Span, hash: u64) -> Self {
        Self {
            source: TextSource { span },
            hash,
        }
    }

    /// Resolve the recorded bytes. The span is guaranteed to target
    /// `interned_text` by `RecordStore::record_text`.
    #[inline]
    pub(crate) fn resolve<'a>(&self, interned_text: &'a InternedText<'_>) -> &'a str {
        self.source.resolve(interned_text)
    }
}

impl TextSource {
    #[inline]
    pub(crate) fn resolve<'a>(self, interned_text: &'a InternedText<'_>) -> &'a str {
        &interned_text.bytes[self.span.range()]
    }
}

impl Hash for RecordedText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::interned_str::{InternedStr, TextEpoch, TextInput};
    use crate::primitives::span::Span;
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
