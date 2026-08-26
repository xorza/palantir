//! The anchoring rule the two deferred-batch columns share.

/// A batch that anchors to a single draw group via its `last_group`
/// index.
///
/// The one thing [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch)
/// and [`GroupBatch`](crate::renderer::render_buffer::group_batch::GroupBatch)
/// genuinely share: each is staged against the last group it reaches
/// into, and the scheduler emits it when that group comes up. Everything
/// else about the two — a text batch's scissor, damage intersection and
/// mask chain — is its own.
pub(crate) trait PerGroupBatch {
    fn last_group(&self) -> usize;
}
