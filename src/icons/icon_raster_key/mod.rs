//! The atlas cache key for one rasterized icon, and the size quantization
//! that bounds how many distinct rasters a continuous zoom can create.

use crate::icons::icon_set::IconRef;
use crate::primitives::num::F32Px;
use glam::{U16Vec2, Vec2};

/// Physical sizes at or below this rasterize at exactly the pixel box asked
/// for. This is where a pixel of size error would show, and where rasters are
/// cheap enough that the churn a zoom gesture creates is affordable — 13-72 µs
/// per icon measured across gradient, mask, and clip-path artwork.
const EXACT_MAX_PX: u32 = 64;

/// Above [`EXACT_MAX_PX`], sizes round to a multiple of this.
///
/// Raster cost climbs with area, so past the exact band a continuous zoom is
/// paying for a fresh raster of every visible icon on every frame that crosses
/// a pixel. Rounding to 4 px cuts that rate by 4x for at most 3% of size error
/// at 64 px and less above — invisible at a size where one pixel is under 2%
/// of the icon.
const COARSE_STEP_PX: u32 = 4;

/// Hard ceiling on either axis of a raster.
///
/// A canvas zoomed far enough would otherwise ask for a 4096 px icon — 64 MB
/// of atlas for one draw. Past the cap the largest cached raster is reused and
/// magnifies, which is the one place icons are not pixel-exact and the one
/// place nobody is looking. Divisible by [`COARSE_STEP_PX`], so the clamp
/// lands on a rung rather than beside one.
const MAX_RASTER_PX: u32 = 512;

/// What one cached icon raster is keyed by: which icon, at what physical pixel
/// size. Ten bytes, against the 24 of cosmic's glyph `CacheKey` — an icon
/// needs no subpixel bins, because unlike a glyph it snaps to whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRasterKey {
    pub(crate) icon: IconRef,
    /// Private, and [`Self::for_box`] is the only way to set it, because
    /// both axes are guaranteed at least 1 — see [`snap_px`] and the
    /// clamp beside it. The icon backend rests on that: an icon always
    /// packs a rectangle, so a slot of its own that owns none is a broken
    /// contract rather than a raster to skip.
    size: U16Vec2,
}

impl IconRasterKey {
    /// The physical pixel box this raster is cached at. Never zero on
    /// either axis.
    pub(crate) fn size(self) -> U16Vec2 {
        self.size
    }

    /// The key for drawing `icon` into a physical-pixel box of `box_px`.
    ///
    /// Snaps through the two-part ladder above, preserving the box's aspect
    /// ratio: the longer axis picks the rung and the shorter one follows it,
    /// so an icon never stretches by a pixel just because its two axes landed
    /// on different rungs.
    pub(crate) fn for_box(icon: IconRef, box_px: Vec2) -> Self {
        debug_assert!(
            box_px.x > 0.0 && box_px.y > 0.0 && box_px.is_finite(),
            "icon raster box must be positive and finite, got {box_px:?}",
        );
        let long = box_px.x.max(box_px.y).max(1.0);
        let target = snap_px(long.fast_round() as u32);
        // Scale from the *unrounded* long axis, so the short one tracks the
        // true aspect rather than the rounding of its sibling.
        let k = target as f32 / long;
        let short = (box_px.x.min(box_px.y) * k)
            .fast_round()
            .clamp(1.0, MAX_RASTER_PX as f32) as u32;
        let size = if box_px.x >= box_px.y {
            U16Vec2::new(target as u16, short as u16)
        } else {
            U16Vec2::new(short as u16, target as u16)
        };
        Self { icon, size }
    }
}

/// One axis through the ladder. Never returns zero — a raster of no pixels has
/// no slot to cache and no quad to draw.
const fn snap_px(px: u32) -> u32 {
    if px <= EXACT_MAX_PX {
        if px == 0 { 1 } else { px }
    } else {
        let stepped = ((px + COARSE_STEP_PX / 2) / COARSE_STEP_PX) * COARSE_STEP_PX;
        if stepped > MAX_RASTER_PX {
            MAX_RASTER_PX
        } else {
            stepped
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::icons::icon_set::IconRef;
    use glam::U16Vec2;

    impl IconRasterKey {
        /// A key at an exact pixel box, for the rasterizer tests that
        /// drive sizes the ladder would not land on.
        pub(crate) fn for_test(icon: IconRef, size: U16Vec2) -> Self {
            Self { icon, size }
        }
    }
}

#[cfg(test)]
mod tests;
