//! One scissor + rounded-clip scope's worth of quads, the unit the
//! backend replays a render pass in.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;

/// A contiguous quad range sharing one clip scope. The composer opens a
/// new group whenever the scissor or the rounded-mask chain changes, so
/// the backend sets clip state once per group and then draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// Outer-to-inner rounded-mask chain in the frame's rounded-clip pool.
    pub(crate) rounded_clips: Span,
    pub(crate) quads: Span,
}
