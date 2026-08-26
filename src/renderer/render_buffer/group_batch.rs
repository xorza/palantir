//! Non-text draw ranges deferred to the group that drains them.

use crate::primitives::span::Span;
use crate::renderer::render_buffer::per_group_batch::PerGroupBatch;

/// A contiguous non-text draw range anchored to the group that drains it.
/// The owning `RenderBuffer` column determines what [`Self::items`] indexes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroupBatch {
    pub(crate) items: Span,
    pub(crate) last_group: u32,
}

impl PerGroupBatch for GroupBatch {
    fn last_group(&self) -> usize {
        self.last_group as usize
    }
}
