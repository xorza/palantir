//! One shaped run's render-handoff identity, from the encoder to the text backend.

use crate::common::span::Span;
use crate::primitives::text::interned_text::InternedText;
use crate::primitives::text::recorded_text::RecordedText;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;

/// One shaped run's handoff identity: the shaped-buffer cache key plus the record-store span of the source bytes it hashes. Minted once by the encoder via [`Self::new`] and carried as a unit so key and bytes can't drift; [`Self::resolve_request`] turns the pair back into a shaping request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShapedTextRef {
    pub(crate) key: TextShapeKey,
    pub(crate) span: Span,
}

impl ShapedTextRef {
    /// Pair a measured cache key with its recorded source. The O(1) hash comparison catches a mis-pairing here; [`Self::resolve_request`] checks the bytes on the way out.
    pub(crate) fn new(key: TextShapeKey, text: &RecordedText) -> Self {
        debug_assert_eq!(
            key.text_hash,
            TextShapeKey::content_hash(text.hash),
            "shaped-text key paired with a different run's source bytes",
        );
        Self {
            key,
            span: text.span,
        }
    }

    /// Resolve the bytes and rebuild the shaping request replayed on an encoded-cache miss. [`TextShapeRequest::for_key`] checks the bytes against the key's content hash, which makes cached-buffer reuse sound. `TextShape::is_noop` drops empty runs before recording, so the `expect` is that contract.
    pub(crate) fn resolve_request<'a>(
        self,
        interned_text: &'a InternedText<'_>,
    ) -> TextShapeRequest<'a> {
        TextShapeRequest::for_key(interned_text.resolve(self.span), self.key)
            .expect("a recorded text run has bytes — `TextShape::is_noop` drops the empty one")
    }
}
