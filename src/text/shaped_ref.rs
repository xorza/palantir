//! One shaped run's render-handoff identity, carried from the encoder to
//! the text backend.

use crate::common::hash;
use crate::primitives::interned_str::{InternedText, RecordedText, TextSource};
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;

/// One shaped run's render-handoff identity: the shaped-buffer cache key
/// plus the record-store span of the exact source bytes it hashes. Minted
/// once by the encoder via [`Self::new`] (which checks the pairing against
/// the recorded content hash) and carried as a unit through the paint
/// payload, composer, and text backend so the key cannot drift from its
/// bytes between layers; [`Self::resolve_request`] is the single place the
/// pair turns back into a shaping request.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapedTextRef {
    pub(crate) key: TextShapeKey,
    pub(crate) source: TextSource,
}

impl ShapedTextRef {
    /// Pair a measured cache key with the recorded source it was shaped
    /// from. The O(1) hash comparison catches a mis-paired key/source at
    /// the only place both sides are still individually known.
    pub(crate) fn new(key: TextShapeKey, text: &RecordedText) -> Self {
        debug_assert_eq!(
            key.text_hash,
            text.hash.max(1),
            "shaped-text key paired with a different run's source bytes",
        );
        Self {
            key,
            source: text.source,
        }
    }

    /// Resolve the retained bytes and rebuild the shaping request the
    /// backend replays on an encoded-cache miss. Debug-checks that the
    /// resolved bytes still hash to the key's content hash — the contract
    /// that makes reusing a cached shaped buffer sound.
    pub(crate) fn resolve_request<'a>(
        self,
        interned_text: &'a InternedText<'_>,
    ) -> TextShapeRequest<'a> {
        let text = self.source.resolve(interned_text);
        debug_assert_eq!(hash::hash_str(text).max(1), self.key.text_hash);
        TextShapeRequest {
            text,
            key: self.key,
        }
    }
}
