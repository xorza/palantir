//! Draw-group and batch scheduling records shared by composer and backend.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// Outer-to-inner rounded-mask chain in the frame's rounded-clip pool.
    pub(crate) rounded_clips: Span,
    pub(crate) quads: Span,
}

/// A coalesced text batch anchored to the final group it contributes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextBatch {
    pub(crate) texts: Span,
    pub(crate) last_group: u32,
    /// Physical-pixel union of every contributing text run's bounds.
    pub(crate) scissor: URect,
    pub(crate) rounded_clips: Span,
}

/// A contiguous non-text draw range anchored to the group that drains it.
/// The owning `RenderBuffer` column determines what [`Self::items`] indexes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroupBatch {
    pub(crate) items: Span,
    pub(crate) last_group: u32,
}

/// Above-text replay tiers in the backend's fixed intra-group order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintTier {
    Mesh,
    Image,
    Curve,
}
