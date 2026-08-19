//! Text as a shape record stores it: where the bytes are, and their hash.

use crate::primitives::span::Span;
use crate::primitives::text_source::TextSource;
use std::hash::{Hash, Hasher};

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

impl RecordedText {
    pub(crate) fn new(span: Span, hash: u64) -> Self {
        Self {
            source: TextSource { span },
            hash,
        }
    }
}

impl Hash for RecordedText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}
