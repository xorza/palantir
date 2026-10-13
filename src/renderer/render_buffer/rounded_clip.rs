//! One rounded mask a clip chain stacks.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;

/// Physical-px rounded-clip geometry for stencil masking. `mask_rect` is the clip's full rect, **not** clamped to viewport or ancestor scissor, so corner curves stay anchored at the true edges when partially off-screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedClip {
    pub(crate) mask_rect: Rect,
    pub(crate) corners: Corners,
}
