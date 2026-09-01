//! The borrowed record-pass text arena that spans resolve against.

use crate::primitives::span::Span;

/// Borrow of the complete record-pass text arena. Recorded text spans
/// resolve against this value; the caller's `Ref<RecordStore>` is what
/// keeps the arena immutable for as long as this lives.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InternedText<'a> {
    bytes: &'a str,
}

impl<'a> InternedText<'a> {
    pub(crate) fn new(bytes: &'a str) -> Self {
        Self { bytes }
    }

    /// The bytes `span` addresses.
    ///
    /// Slicing lives here rather than on the span, because the arena is
    /// the half that knows what a span means — and a `Span` from any
    /// other arena resolving here would be the bug the record pass
    /// rebases handles to avoid.
    #[inline]
    pub(crate) fn resolve(self, span: Span) -> &'a str {
        &self.bytes[span.range()]
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::primitives::interned_text::InternedText;

    impl<'a> InternedText<'a> {
        /// The whole arena, for a snapshot comparing two record passes
        /// byte for byte. Production resolves spans and never wants the
        /// buffer itself.
        pub(crate) fn all(self) -> &'a str {
            self.bytes
        }
    }
}
