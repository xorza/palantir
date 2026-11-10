//! Coalesced glyph draws deferred to the last group they reach into.

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;

/// A coalesced text batch anchored to the final group it contributes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextBatch {
    pub(crate) texts: Span,
    pub(crate) last_group: u32,
    /// Physical-pixel union of every contributing text run's bounds.
    pub(crate) scissor: URect,
}
