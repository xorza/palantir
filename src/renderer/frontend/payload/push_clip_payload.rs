//! The clip-scope push the encoder hands the sink.

use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;

/// Scissor clip payload. `corners` is all-zero for plain rect clips
/// and non-zero for rounded-mask clips — the composer decides which
/// path to take by inspecting it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PushClipPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
}

impl PushClipPayload {
    /// A plain rect clip — zero corners, which is what tells the
    /// composer to take the scissor path rather than the rounded mask.
    pub(crate) fn rect(rect: Rect) -> Self {
        Self {
            rect,
            corners: Corners::ZERO,
        }
    }

    pub(crate) fn rounded(rect: Rect, corners: Corners) -> Self {
        Self { rect, corners }
    }
}
