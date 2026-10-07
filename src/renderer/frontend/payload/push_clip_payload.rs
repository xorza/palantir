//! The clip-scope push the encoder hands the sink.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;

/// Scissor clip payload. `corners` is all-zero for plain rect clips; non-zero selects the composer's rounded-mask path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PushClipPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
}

impl PushClipPayload {
    /// A plain rect clip: zero corners select the scissor path.
    pub(crate) const fn rect(rect: Rect) -> Self {
        Self {
            rect,
            corners: Corners::ZERO,
        }
    }
}
