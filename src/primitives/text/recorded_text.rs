//! Text as a shape record stores it: where the bytes are, and their hash.

use crate::common::span::Span;
use std::hash::{Hash, Hasher};

/// Text stored on a [`ShapeRecord`](crate::shape::record::ShapeRecord).
/// Its span always addresses the active record store because lowering rebases
/// handles from any other arena before constructing this value.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RecordedText {
    pub(crate) span: Span,
    /// `hash_str` of the recorded bytes, computed once at record time.
    /// Downstream consumers (scene identity, [`crate::text::key::TextShapeKey`])
    /// reuse it instead of rescanning the text.
    pub(crate) hash: u64,
}

impl RecordedText {
    pub(crate) const fn new(span: Span, hash: u64) -> Self {
        Self { span, hash }
    }
}

impl Hash for RecordedText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}
