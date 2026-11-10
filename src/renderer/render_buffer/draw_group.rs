//! One scissor scope's quads, the unit the backend replays a pass in.

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;

/// A contiguous quad range sharing one scissor; the composer opens a new group when the scissor
/// changes, so the backend sets it once per group. A rounded clip rides each draw's instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    pub(crate) quads: Span,
}
