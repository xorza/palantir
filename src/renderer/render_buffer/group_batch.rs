//! Non-text draw ranges deferred to the group that drains them.

use crate::primitives::span::Span;

/// A contiguous non-text draw range anchored to the group that drains it.
/// The owning `RenderBuffer` column determines what [`Self::items`] indexes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroupBatch {
    pub(crate) items: Span,
    pub(crate) last_group: u32,
}
