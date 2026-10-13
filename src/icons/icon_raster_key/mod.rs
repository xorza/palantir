//! The atlas cache key for one rasterized icon, and the size quantization that bounds distinct rasters under continuous zoom.

use crate::primitives::identity::icon_ref::IconRef;
use crate::primitives::math::num::F32Px;
use glam::{U16Vec2, Vec2};

/// Physical sizes at or below this rasterize at exactly the pixel box asked for: size error shows here, and rasters are cheap.
const EXACT_MAX_PX: u16 = 64;

/// Above [`EXACT_MAX_PX`], sizes round to a multiple of this, so a continuous zoom does not pay a fresh raster per icon per pixel crossed (at most 3% size error at 64 px).
const COARSE_STEP_PX: u16 = 4;

/// Hard ceiling on either axis of a raster (4096 px would cost 64 MB of atlas); past it the largest cached raster is magnified. Divisible by [`COARSE_STEP_PX`], so the clamp lands on a rung.
const MAX_RASTER_PX: u16 = 512;

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
        self.size.max_element() <= EXACT_MAX_PX
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
        let k = f32::from(target) / long;
        let short = (box_px.x.min(box_px.y) * k)
            .fast_round()
            .clamp(1.0, f32::from(MAX_RASTER_PX)) as u16;
        let size = if box_px.x >= box_px.y {
            U16Vec2::new(target, short)
        } else {
            U16Vec2::new(short, target)
        };
        Self { icon, size }
    }
}

/// One axis through the ladder; never zero. A box past `u32::MAX` px arrives saturated, so the rounding add saturates too and the rung clamps to the ceiling.
fn snap_px(px: u32) -> u16 {
    let step = u32::from(COARSE_STEP_PX);
    let rung = if px <= u32::from(EXACT_MAX_PX) {
        px.max(1)
    } else {
        (px.saturating_add(step / 2) / step * step).min(u32::from(MAX_RASTER_PX))
    };
    u16::try_from(rung).expect("the ladder tops out at MAX_RASTER_PX")
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::primitives::identity::icon_ref::IconRef;
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
