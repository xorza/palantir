//! One shaped run's render-handoff identity, carried from the encoder to
//! the text backend.

use crate::primitives::interned_text::InternedText;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::span::Span;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;

/// One shaped run's render-handoff identity: the shaped-buffer cache key
/// plus the record-store span of the exact source bytes it hashes. Minted
/// once by the encoder via [`Self::new`] (which checks the pairing against
/// the recorded content hash) and carried as a unit through the paint
/// payload, composer, and text backend so the key cannot drift from its
/// bytes between layers; [`Self::resolve_request`] is the single place the
/// pair turns back into a shaping request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShapedTextRef {
    pub(crate) key: TextShapeKey,
    pub(crate) span: Span,
}

impl ShapedTextRef {
    /// Pair a measured cache key with the recorded source it was shaped
    /// from. The O(1) hash comparison catches a mis-paired key/source
    /// here, while the recorded hash is still to hand and nothing has to
    /// re-read the bytes; [`Self::resolve_request`] checks the resolved
    /// bytes themselves on the way back out.
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

    /// Resolve the retained bytes and rebuild the shaping request the
    /// backend replays on an encoded-cache miss.
    ///
    /// [`TextShapeRequest::for_key`] is what checks the resolved bytes
    /// against the key's content hash — the contract that makes reusing a
    /// cached shaped buffer sound, and the reason this is a pairing call
    /// rather than a struct literal.
    ///
    /// A run that reaches here has bytes: an empty one shapes no buffer,
    /// so it carries [`TextShapeKey::INVALID`] and the backend drops it
    /// before asking. The `expect` is that contract, not a case to answer.
    pub(crate) fn resolve_request<'a>(
        self,
        interned_text: &'a InternedText<'_>,
    ) -> TextShapeRequest<'a> {
        TextShapeRequest::for_key(interned_text.resolve(self.span), self.key)
            .expect("a run with a shaped buffer has bytes — filter INVALID keys first")
    }
}
