//! One rounded clip a clip chain stacks.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use glam::Vec2;

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

    /// The signed distance of `p` to the clip's edge, negative inside: the CPU twin of
    /// `palantir_sdf_rounded_rect`, the same f32 steps, each corner's radius picked by the side of
    /// the center `p` lies on.
    pub(crate) fn distance(self, p: Vec2) -> f32 {
        let half = Vec2::from(self.mask_rect.size) * 0.5;
        let q = p - self.mask_rect.min - half;
        let [tl, tr, br, bl] = self.corners.to_array();
        let r = match (q.x >= 0.0, q.y >= 0.0) {
            (false, false) => tl,
            (true, false) => tr,
            (true, true) => br,
            (false, true) => bl,
        };
        let d = q.abs() - (half - Vec2::splat(r));
        d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length() - r
    }

    /// Whether every point of `rect` lies at least the antialiasing reach and [`Self::SLACK_PX`]
    /// inside the clip, so the clip's coverage there is whole. The rounded rect is convex and so is
    /// the set of points that far inside it, which holds `rect` when it holds its four corners.
    pub(crate) fn holds(self, rect: Rect) -> bool {
        let depth = AA_HALF_WIDTH + Self::SLACK_PX;
        // A rect past the straight edges' depth is not held, which most draws across an edge are:
        // four compares before any distance.
        if !self.mask_rect.deflated(depth).contains_rect(rect) {
            return false;
        }
        let max = rect.max();
        [
            rect.min,
            Vec2::new(max.x, rect.min.y),
            max,
            Vec2::new(rect.min.x, max.y),
        ]
        .into_iter()
        .all(|corner| self.distance(corner) <= -depth)
    }
}

#[cfg(feature = "internals")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
