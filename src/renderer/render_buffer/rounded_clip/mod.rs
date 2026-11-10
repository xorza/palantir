//! One rounded clip a clip chain stacks.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;

/// Physical-px rounded-clip geometry, which the fragment stage reads as `PalantirClip`
/// (`primitives/wgsl/frame.wgsl`), uploaded as it is. `mask_rect` is the clip's full rect, **not**
/// clamped to viewport or ancestor scissor, so corner curves stay anchored at the true edges when
/// partially off-screen.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RoundedClip {
    pub(crate) mask_rect: Rect,
    pub(crate) corners: Corners,
}

impl RoundedClip {
    /// How far past the antialiasing reach a point stands before the CPU counts the clip's
    /// coverage there as whole: the GPU's f32 distance to the rect, whose coordinates lie below
    /// 2^16 px, parts from the CPU's by under 1/128 px, as `ComposeSession::DECAL_SLACK_PX`
    /// states for an image's edge.
    const SLACK_PX: f32 = 1.0 / 64.0;

    /// A rect every pixel center of which this clip leaves whole: inside the rect its arcs
    /// inscribe, by the antialiasing reach and [`Self::SLACK_PX`]. The rect lies inside the
    /// rounded rect, so a point inside it by a distance lies inside the rounded rect by as much.
    pub(crate) fn covered(self) -> Rect {
        self.mask_rect
            .inscribed_for_corners(self.corners)
            .deflated(AA_HALF_WIDTH + Self::SLACK_PX)
    }
}

#[cfg(feature = "internals")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
