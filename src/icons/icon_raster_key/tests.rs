use crate::icons::icon_raster_key::{IconRasterKey, MAX_RASTER_PX};
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::{IconHandle, IconRef};
use glam::{U16Vec2, Vec2};

fn icon() -> IconRef {
    IconRef::fixture(0, 0)
}

fn size(w: f32, h: f32) -> U16Vec2 {
    IconRasterKey::for_box(icon(), Vec2::new(w, h)).size()
}

/// Pins the sizes doc comments quote (they went stale when [`IconSetId`] gained a generation). Each is the sum of the one below: id = slot + generation, ref = id + icon index, key = ref + `u16` box. Nothing is padded; a key is hashed per icon draw.
#[test]
fn the_key_chain_is_as_wide_as_its_docs_claim() {
    assert_eq!(size_of::<IconSetId>(), 4, "two u16s");
    assert_eq!(size_of::<IconRef>(), 6, "an id plus an icon index");
    assert_eq!(size_of::<IconRasterKey>(), 10, "a ref plus a U16Vec2");
    assert_eq!(
        size_of::<IconHandle>(),
        16,
        "a ref plus a Vec2, aligned to 4"
    );
}

/// Hand-computed rungs, notably just above the exact band where a plain `round` disagrees with the ladder.
#[test]
fn ladder_is_exact_to_64_then_steps_by_four() {
    // 24 logical px at 1.5 scale is 36 physical, inside the exact band: drawn at exactly 36.
    assert_eq!(size(36.0, 36.0), U16Vec2::new(36, 36));
    assert_eq!(size(1.0, 1.0), U16Vec2::new(1, 1));
    assert_eq!(size(64.0, 64.0), U16Vec2::new(64, 64), "band edge is exact");
    // 50 logical px at 1.5 is 75 physical, past the band: rounds to the 4 px grid, 75/4 = 18.75 → 19 → 76.
    assert_eq!(size(75.0, 75.0), U16Vec2::new(76, 76));
    // Rounds to the nearest rung, so 65 and 66 land either side.
    assert_eq!(size(65.0, 65.0), U16Vec2::new(64, 64));
    assert_eq!(size(66.0, 66.0), U16Vec2::new(68, 68));
    assert_eq!(size(100.0, 100.0), U16Vec2::new(100, 100), "already a rung");
    assert_eq!(size(102.0, 102.0), U16Vec2::new(104, 104));
}

/// The ladder must not stretch an icon: the short axis follows the rung the long axis picked.
#[test]
fn aspect_survives_the_coarse_band() {
    // 2:1 on a rung: unchanged.
    assert_eq!(size(100.0, 50.0), U16Vec2::new(100, 50));
    // 2:1 off a rung: the long axis rounds 102 → 104 and the short scales with it (51 × 104/102 = 52.0), not stays 51 (2.0 → 2.04).
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

/// A sub-pixel box still gets a slot; a zero-pixel raster can't be allocated or drawn.
#[test]
fn subpixel_boxes_round_up_to_one() {
    assert_eq!(size(0.4, 0.4), U16Vec2::new(1, 1));
    assert_eq!(size(20.0, 0.2), U16Vec2::new(20, 1));
}
