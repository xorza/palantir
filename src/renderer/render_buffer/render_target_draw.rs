//! Off-screen `GpuView` targets a frame paints, and the roster that keeps them alive.

use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use glam::UVec2;

/// One `GpuView` off-screen target to paint this frame (see
/// [`RenderBuffer::frame_targets`](crate::renderer::render_buffer::RenderBuffer::frame_targets)).
/// The backend allocates it to [`Self::capacity`] and runs `paint` before the
/// main pass samples it.
#[derive(Clone, Debug)]
pub(crate) struct RenderTargetDraw {
    pub(crate) id: TextureId,
    /// What the target holds: the on-screen part of the view.
    pub(crate) used: UVec2,
    /// What the whole view measures, on and off screen, since layout may overflow.
    pub(crate) full: UVec2,
    /// Where `used` begins within `full`.
    pub(crate) offset: UVec2,
    pub(crate) raster_scale: f32,
    pub(crate) paint: GpuPaintRef,
    /// The view's repaint version; see [`ViewStamp`].
    pub(crate) epoch: u64,
}

/// Everything a painted target's pixels depend on besides the callback. A
/// target whose last paint carries the same stamp needs no repaint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewStamp {
    epoch: u64,
    used: UVec2,
    full: UVec2,
    offset: UVec2,
    display_scale: f32,
    raster_scale: f32,
}

/// The smallest bucket, Skia's `kMinScratchTextureSize`.
const MIN_BUCKET_PX: u32 = 16;

/// Past this, buckets step by half an octave rather than a whole one, as Skia's `kMagicTol`.
const FINE_BUCKETS_PAST_PX: u32 = 1024;

impl RenderTargetDraw {
    /// The texture size `used` is painted into, per axis: exact where the whole view shows, a
    /// bucket where it is clipped. A clipped axis follows a scroll every frame, and a bucket
    /// holds still across it, so a scroll reallocates about twice per octave instead of every
    /// frame. Never past `full`, which is all the view can show.
    pub(crate) fn capacity(&self) -> UVec2 {
        let axis = |used: u32, full: u32| {
            if used < full {
                bucket(used).min(full)
            } else {
                used
            }
        };
        UVec2::new(
            axis(self.used.x, self.full.x),
            axis(self.used.y, self.full.y),
        )
    }

    pub(crate) const fn stamp(&self, display_scale: f32) -> ViewStamp {
        ViewStamp {
            epoch: self.epoch,
            used: self.used,
            full: self.full,
            offset: self.offset,
            display_scale,
            raster_scale: self.raster_scale,
        }
    }
}

/// Skia's approximate fit (`GrResourceProvider::MakeApprox`): the next power of two, and past
/// [`FINE_BUCKETS_PAST_PX`] also the one-and-a-half rung below it, so at most a third of an axis
/// is slack there.
fn bucket(px: u32) -> u32 {
    let px = px.max(MIN_BUCKET_PX);
    let ceil = px.next_power_of_two();
    if px <= FINE_BUCKETS_PAST_PX {
        return ceil;
    }
    let mid = ceil / 2 + ceil / 4;
    if px <= mid { mid } else { ceil }
}

/// The frame's two views of its `GpuView`s: [`Self::draws`] were composited, and the backend
/// repaints each whose stamp changed; [`Self::live`] still exists. Retention follows the second.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameViews<'a> {
    /// The views composited this frame, each repainted only when its stamp changed.
    pub(crate) draws: &'a [RenderTargetDraw],
    /// Every view recorded, painted or not: the retention roster.
    pub(crate) live: &'a [TextureId],
    /// The display scale every draw is painted at.
    pub(crate) display_scale: f32,
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::texture_id::TextureId;
    use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
    use crate::renderer::render_buffer::render_target_draw::{RenderTargetDraw, bucket};
    use glam::UVec2;

    /// The ladder: 16 at the floor, powers of two up to 1024, then each octave split at its
    /// 1.5 rung (1024 · 1.5 = 1536, 2048 · 1.5 = 3072).
    #[test]
    fn bucket_follows_the_approximate_fit_ladder() {
        let cases = [
            (1, 16),
            (16, 16),
            (17, 32),
            (100, 128),
            (513, 1024),
            (1024, 1024),
            (1025, 1536),
            (1536, 1536),
            (1537, 2048),
            (2049, 3072),
            (3073, 4096),
        ];
        for (px, expected) in cases {
            assert_eq!(bucket(px), expected, "{px}");
        }
    }

    /// A clipped axis buckets, capped at the whole view; a whole axis stays exact.
    #[test]
    fn capacity_buckets_only_a_clipped_axis() {
        let draw = |used: UVec2, full: UVec2| RenderTargetDraw {
            id: TextureId(1),
            used,
            full,
            offset: UVec2::ZERO,
            raster_scale: 1.0,
            paint: GpuPaintRef::noop(),
            epoch: 0,
        };
        let cases = [
            (
                UVec2::new(300, 200),
                UVec2::new(300, 200),
                UVec2::new(300, 200),
            ),
            (
                UVec2::new(300, 100),
                UVec2::new(300, 900),
                UVec2::new(300, 128),
            ),
            (
                UVec2::new(300, 600),
                UVec2::new(300, 900),
                UVec2::new(300, 900),
            ),
            (
                UVec2::new(17, 1100),
                UVec2::new(40, 3000),
                UVec2::new(32, 1536),
            ),
        ];
        for (used, full, expected) in cases {
            assert_eq!(draw(used, full).capacity(), expected, "{used} of {full}");
        }
    }
}
