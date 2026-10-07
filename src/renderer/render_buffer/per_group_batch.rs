//! The anchoring rule the two deferred-batch columns share.

/// A batch anchored to a single draw group via its `last_group` index.
///
/// What [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch) and [`GroupBatch`](crate::renderer::render_buffer::group_batch::GroupBatch) share: each is staged against the last group it reaches into and emitted when that group comes up. Everything else is its own.
pub(crate) trait PerGroupBatch {
    fn last_group(&self) -> usize;
}
