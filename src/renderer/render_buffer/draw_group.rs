//! One scissor and rounded-clip scope's quads, the unit the backend replays a pass in.

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;

/// A contiguous quad range sharing one clip scope; the composer opens a new group when the scissor or rounded-mask chain changes, so the backend sets clip state once per group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// Outer-to-inner rounded-mask chain in the frame's rounded-clip pool.
    pub(crate) rounded_clips: Span,
    pub(crate) quads: Span,
}
