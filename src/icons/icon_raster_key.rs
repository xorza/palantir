use crate::icons::icon_set::IconRef;
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
pub(crate) const MAX_RASTER_PX: u32 = 512;

/// What one cached icon raster is keyed by: which icon, at what physical pixel
/// size. Eight bytes, against the 24 of cosmic's glyph `CacheKey` — an icon
/// needs no subpixel bins, because unlike a glyph it snaps to whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconRasterKey {
    pub(crate) icon: IconRef,
    pub(crate) size: U16Vec2,
}

impl IconRasterKey {
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
        let target = snap_px(long.round() as u32);
        // Scale from the *unrounded* long axis, so the short one tracks the
        // true aspect rather than the rounding of its sibling.
        let k = target as f32 / long;
        let short = (box_px.x.min(box_px.y) * k)
            .round()
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
mod tests {
    use crate::icons::icon_atlas::IconId;
    use crate::icons::icon_raster_key::{IconRasterKey, MAX_RASTER_PX};
    use crate::icons::icon_registry::IconSetId;
    use crate::icons::icon_set::IconRef;
    use glam::{U16Vec2, Vec2};

    fn icon() -> IconRef {
        IconRef {
            set: IconSetId(0),
            icon: IconId(0),
        }
    }

    fn size(w: f32, h: f32) -> U16Vec2 {
        IconRasterKey::for_box(icon(), Vec2::new(w, h)).size
    }

    /// Hand-computed rungs. The interesting ones are just above the exact
    /// band, where a plain `round` would disagree with the ladder.
    #[test]
    fn ladder_is_exact_to_64_then_steps_by_four() {
        // 24 logical px at 1.5 display scale is 36 physical — inside the
        // exact band, so it is drawn at exactly 36.
        assert_eq!(size(36.0, 36.0), U16Vec2::new(36, 36));
        assert_eq!(size(1.0, 1.0), U16Vec2::new(1, 1));
        assert_eq!(size(64.0, 64.0), U16Vec2::new(64, 64), "band edge is exact");
        // 50 logical px at 1.5 scale is 75 physical. Past the band, so it
        // rounds to the 4 px grid: 75/4 = 18.75 -> 19 -> 76, not 75.
        assert_eq!(size(75.0, 75.0), U16Vec2::new(76, 76));
        // Rounding is to *nearest* rung, so 65 and 66 land either side.
        assert_eq!(size(65.0, 65.0), U16Vec2::new(64, 64));
        assert_eq!(size(66.0, 66.0), U16Vec2::new(68, 68));
        assert_eq!(size(100.0, 100.0), U16Vec2::new(100, 100), "already a rung");
        assert_eq!(size(102.0, 102.0), U16Vec2::new(104, 104));
    }

    /// The ladder must not stretch an icon: the short axis follows the rung
    /// the long axis picked, rather than being snapped on its own.
    #[test]
    fn aspect_survives_the_coarse_band() {
        // 2:1 already on a rung — unchanged, and exactly 2:1.
        assert_eq!(size(100.0, 50.0), U16Vec2::new(100, 50));
        // 2:1 off a rung: the long axis rounds 102 -> 104, and the short one
        // scales with it (51 * 104/102 = 52.0) instead of staying at 51,
        // which would have made it 2.0 -> 2.04.
        assert_eq!(size(102.0, 51.0), U16Vec2::new(104, 52));
        // Same, tall rather than wide.
        assert_eq!(size(51.0, 102.0), U16Vec2::new(52, 104));
        // Inside the exact band nothing moves at all.
        assert_eq!(size(48.0, 24.0), U16Vec2::new(48, 24));
    }

    #[test]
    fn oversize_boxes_clamp_to_the_ceiling_and_keep_aspect() {
        assert_eq!(
            size(4096.0, 4096.0),
            U16Vec2::splat(MAX_RASTER_PX as u16),
            "a deeply zoomed canvas must not ask for a 64 MB raster",
        );
        // 4:1 at the ceiling: long axis clamps, short axis follows.
        assert_eq!(size(2048.0, 512.0), U16Vec2::new(512, 128));
    }

    /// A sub-pixel box still has to produce a slot — a raster of zero pixels
    /// could not be allocated or drawn.
    #[test]
    fn subpixel_boxes_round_up_to_one() {
        assert_eq!(size(0.4, 0.4), U16Vec2::new(1, 1));
        assert_eq!(size(20.0, 0.2), U16Vec2::new(20, 1));
    }
}
