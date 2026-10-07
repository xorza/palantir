//! The atlas cache key for one rasterized icon, and the size quantization that bounds distinct rasters under continuous zoom.

use crate::icons::icon_set::IconRef;
use crate::primitives::math::num::F32Px;
use glam::{U16Vec2, Vec2};

/// Physical sizes at or below this rasterize at exactly the pixel box asked for: size error shows here, and rasters are cheap.
const EXACT_MAX_PX: u32 = 64;

/// Above [`EXACT_MAX_PX`], sizes round to a multiple of this, so a continuous zoom does not pay a fresh raster per icon per pixel crossed (at most 3% size error at 64 px).
const COARSE_STEP_PX: u32 = 4;

/// Hard ceiling on either axis of a raster (4096 px would cost 64 MB of atlas); past it the largest cached raster is magnified. Divisible by [`COARSE_STEP_PX`], so the clamp lands on a rung.
const MAX_RASTER_PX: u32 = 512;

/// What one cached icon raster is keyed by: icon and physical pixel size. Ten bytes against cosmic's 24, since an icon snaps to whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRasterKey {
    pub(crate) icon: IconRef,
    /// Private: [`Self::for_box`] is the only setter, so both axes are at least 1, which the icon backend relies on to always pack a rectangle.
    size: U16Vec2,
}

impl IconRasterKey {
    pub(crate) const fn size(self) -> U16Vec2 {
        self.size
    }

    pub(crate) fn is_exact(self) -> bool {
        u32::from(self.size.max_element()) <= EXACT_MAX_PX
    }

    /// The key for drawing `icon` into a physical-pixel box of `box_px`: the longer axis picks the rung and the shorter follows, preserving aspect.
    #[expect(
        clippy::cast_sign_loss,
        reason = "the box is asserted positive and finite above, and its long axis is held at 1 or more"
    )]
    pub(crate) fn for_box(icon: IconRef, box_px: Vec2) -> Self {
        debug_assert!(
            box_px.x > 0.0 && box_px.y > 0.0 && box_px.is_finite(),
            "icon raster box must be positive and finite, got {box_px:?}",
        );
        let long = box_px.x.max(box_px.y).max(1.0);
        let target = snap_px(long.fast_round() as u32);
        // Scale from the unrounded long axis, so the short one tracks the true aspect.
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

/// One axis through the ladder; never zero.
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
pub(crate) mod internals {
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::icons::icon_set::IconRef;
    use glam::U16Vec2;

    impl IconRasterKey {
        /// A key at an exact pixel box, for tests that drive sizes the ladder would not land on.
        pub(crate) fn for_test(icon: IconRef, size: U16Vec2) -> Self {
            Self { icon, size }
        }
    }
}

#[cfg(test)]
mod tests;
