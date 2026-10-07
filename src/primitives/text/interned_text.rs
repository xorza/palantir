//! The borrowed record-pass text arena that spans resolve against.

use crate::common::span::Span;

/// Borrow of the complete record-pass text arena that recorded spans resolve against; the caller's `Ref<RecordStore>` keeps it immutable.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InternedText<'a> {
    bytes: &'a str,
}

impl<'a> InternedText<'a> {
    pub(crate) const fn new(bytes: &'a str) -> Self {
        Self { bytes }
    }

    /// The bytes `span` addresses; a `Span` from another arena resolving here is the bug the record pass rebases handles to avoid.
    #[inline]
    pub(crate) fn resolve(self, span: Span) -> &'a str {
        &self.bytes[span.range()]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::text::interned_text::InternedText;

    impl<'a> InternedText<'a> {
        /// The whole arena, for snapshot-comparing record passes.
        pub(crate) fn all(self) -> &'a str {
            self.bytes
        }
    }
}
