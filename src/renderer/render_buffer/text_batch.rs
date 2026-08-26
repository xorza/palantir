//! Coalesced glyph draws deferred to the last group they reach into.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::per_group_batch::PerGroupBatch;

/// A coalesced text batch anchored to the final group it contributes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextBatch {
    pub(crate) texts: Span,
    pub(crate) last_group: u32,
    /// Physical-pixel union of every contributing text run's bounds.
    pub(crate) scissor: URect,
    pub(crate) rounded_clips: Span,
}

impl PerGroupBatch for TextBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}
