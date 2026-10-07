//! DamageEngine visualization: a static scene rendered twice into one Harness (so
//! `DamageEngine.prev` carries over), the second with
//! `DebugOverlayConfig::dim_undamaged`. Magenta is the clear colour of **both**
//! frames, so a pixel reads magenta only if the scene left it uncovered. A failing
//! test writes its second frame under `tests/visual/output/damage_<name>/`.

use glam::{UVec2, Vec2};
use palantir::golden::image::{Rgba, RgbaImage};
use palantir::{
    Background, Block, Button, Configure, DebugOverlayConfig, FramePaint, Panel, RgbaF32, Sizing,
};

use crate::goldens::{KeptOnFailure, assert_same};
use crate::harness::Harness;

const VIS_CLEAR: RgbaF32 = RgbaF32::srgb(1.0, 0.0, 1.0);

fn count_pixels(img: &RgbaImage, predicate: impl Fn(u8, u8, u8) -> bool) -> u32 {
    img.pixels()
        .filter(|p| {
            let Rgba([r, g, b, _]) = **p;
            predicate(r, g, b)
        })
        .count() as u32
}

const fn is_magenta(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b > 240
}
const fn is_red(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b < 16
}

/// Physical px of the damage overlay's stroke at scale 1
/// (`DAMAGE_OVERLAY_STROKE_WIDTH`).
const STROKE: i32 = 2;
const GAP: i32 = 1;

#[derive(Clone, Copy, Debug)]
struct PxRect {
    min: (i32, i32),
    max: (i32, i32),
}

impl PxRect {
    const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            min: (x, y),
            max: (x + w, y + h),
        }
    }

    fn outline_covers(self, x: i32, y: i32) -> bool {
        let (x0, y0) = (self.min.0 - GAP, self.min.1 - GAP);
        let (x1, y1) = (self.max.0 + GAP, self.max.1 + GAP);
        let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
        let interior =
            (x0 + STROKE..x1 - STROKE).contains(&x) && (y0 + STROKE..y1 - STROKE).contains(&y);
        inside && !interior
    }
}

fn assert_outlines_exactly(img: &RgbaImage, damage: &[PxRect]) {
    for (x, y, p) in img.enumerate_pixels() {
        let Rgba([r, g, b, _]) = *p;
        let (x, y) = (x as i32, y as i32);
        let expected = damage.iter().any(|rect| rect.outline_covers(x, y));
        assert_eq!(
            is_red(r, g, b),
            expected,
            "pixel ({x}, {y}) rgb ({r}, {g}, {b}): outline of {damage:?} expected {expected}",
        );
    }
}

/// The rect the red pixels bracket, for a damage rect the fixture cannot derive.
fn outlined_rect(img: &RgbaImage) -> PxRect {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y, p) in img.enumerate_pixels() {
        let Rgba([r, g, b, _]) = *p;
        if is_red(r, g, b) {
            let (x, y) = (x as i32, y as i32);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    assert!(x0 <= x1, "no red pixel at all: the overlay drew nothing");
    PxRect {
        min: (x0 + GAP, y0 + GAP),
        max: (x1 + 1 - GAP, y1 + 1 - GAP),
    }
}

fn button_scene(
    id_salt: &'static str,
    label: &'static str,
) -> impl FnMut(&mut palantir::Ui) + Copy {
    move |ui: &mut palantir::Ui| {
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .gap(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: RgbaF32::srgb(0.15, 0.15, 0.18).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                Button::new().id_salt(id_salt).label(label).show(ui);
            });
    }
}

fn corner_pair_scene(
    tl_label: &'static str,
    br_label: &'static str,
) -> impl FnMut(&mut palantir::Ui) + Copy {
    move |ui: &mut palantir::Ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: RgbaF32::srgb(0.15, 0.15, 0.18).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                Block::new()
                    .id_salt(("tl", tl_label))
                    .position(Vec2::new(0.0, 0.0))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .background(Background {
                        fill: RgbaF32::srgb(0.2, 0.7, 0.4).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Block::new()
                    .id_salt(("br", br_label))
                    .position(Vec2::new(180.0, 180.0))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .background(Background {
                        fill: RgbaF32::srgb(0.7, 0.3, 0.2).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    }
}

/// Two identical frames of a static scene: frame 2's diff is empty, so the renderer
/// gets no plan and the target must read back as frame 1. Zero magenta shows the
/// scene covered every pixel.
#[test]
fn static_scene_repeats_clean() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);
    let scene = button_scene("hi", "hello");

    let f1 = h.size(size).clear(VIS_CLEAR).frame(scene).image;
    let repeat = h
        .size(size)
        .clear(VIS_CLEAR)
        .overlay(DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        })
        .frame(scene);
    assert_eq!(repeat.paint, FramePaint::Skip);
    let f2 = repeat.image;
    let _kept = KeptOnFailure::new("damage_static_scene_repeats_clean", &f2);

    let painted = count_pixels(&f2, |r, g, b| !is_magenta(r, g, b));
    let total = size.x * size.y;
    assert_eq!(
        painted, total,
        "the scene covers the surface, so no pixel may read as the clear \
         colour. Got {painted}/{total} non-magenta pixels."
    );
    assert_same("damage_static_repeat", &f2, &f1);
}

/// A button label flips from "a" to "b": the damage stays the button's rect, so the
/// frame repaints partially (target loaded, not cleared).
#[test]
fn single_button_change_repaints_partially() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);

    let _f1 = h
        .size(size)
        .clear(VIS_CLEAR)
        .frame(button_scene("b", "a"))
        .image;
    let changed = h
        .size(size)
        .clear(VIS_CLEAR)
        .overlay(DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        })
        .frame(button_scene("b", "b"));
    assert_eq!(changed.paint, FramePaint::Partial);
    let f2 = changed.image;
    let _kept = KeptOnFailure::new("damage_single_button_change_repaints_partially", &f2);

    let painted = count_pixels(&f2, |r, g, b| !is_magenta(r, g, b));
    assert_eq!(
        painted,
        size.x * size.y,
        "a partial frame loads the target, so nothing reads as the clear"
    );
}

/// Smoke pin: `DebugOverlayConfig::damage_rect` puts red stroke pixels on the
/// swapchain; no other test exercises the post-copy pass.
#[test]
fn damage_rect_overlay_strokes_dirty_region() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);

    let _f1 = h.size(size).frame(button_scene("c", "a")).image;
    let f2 = h
        .size(size)
        .overlay(DebugOverlayConfig {
            damage_rect: true,
            ..Default::default()
        })
        .frame(button_scene("c", "b"))
        .image;
    let _kept = KeptOnFailure::new("damage_rect_overlay_strokes_dirty_region", &f2);

    // The label width sets the damage rect's size, so read it back; its corner is
    // the 12 px padding.
    let damage = outlined_rect(&f2);
    assert_eq!(damage.min, (12, 12), "the button sits at the padding");
    assert_outlines_exactly(&f2, &[damage]);
}

/// The workload multi-rect damage exists for: two tiny corner blocks change in a
/// static canvas. A single union rect would span the canvas and escalate to
/// `Damage::Full`; multiple rects stay disjoint, each scissored to its own pass.
/// With `dim_undamaged` the centre reads as frame 1 darkened, never the clear
/// colour. Pinned: (1) zero magenta in the centre, (2) the centre darker than frame
/// 1, (3) top-left green-dominant and bottom-right red-dominant.
#[test]
fn corner_pair_change_keeps_center_unpainted() {
    let mut h = Harness::new();
    let size = UVec2::new(200, 200);

    let f1 = h
        .size(size)
        .clear(VIS_CLEAR)
        .frame(corner_pair_scene("a", "a"))
        .image;
    let f2 = h
        .size(size)
        .clear(VIS_CLEAR)
        .overlay(DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        })
        .frame(corner_pair_scene("b", "b"))
        .image;
    let _kept = KeptOnFailure::new("damage_corner_pair_change_keeps_center_unpainted", &f2);

    let centre_total: u32 = 100 * 100;
    let mut centre_magenta = 0u32;
    let mut f1_lum: u64 = 0;
    let mut f2_lum: u64 = 0;
    for y in 50..150 {
        for x in 50..150 {
            let Rgba([r1, g1, b1, _]) = *f1.get_pixel(x, y);
            let Rgba([r2, g2, b2, _]) = *f2.get_pixel(x, y);
            f1_lum += u64::from(r1) + u64::from(g1) + u64::from(b1);
            f2_lum += u64::from(r2) + u64::from(g2) + u64::from(b2);
            if is_magenta(r2, g2, b2) {
                centre_magenta += 1;
            }
        }
    }
    assert_eq!(
        centre_magenta, 0,
        "centre 100×100 must be free of magenta (got {centre_magenta}/{centre_total}) — \
         dim_undamaged no longer fires LoadOp::Clear, only a translucent dim pass",
    );
    assert!(
        f2_lum < f1_lum,
        "dim pre-pass should darken the centre: got f1_lum={f1_lum}, f2_lum={f2_lum}",
    );

    let Rgba([tl_r, tl_g, tl_b, _]) = *f2.get_pixel(5, 5);
    assert!(
        tl_g > tl_r && tl_g > tl_b,
        "top-left corner pixel should be green-dominant (its painted \
         fill 0.2/0.7/0.4), got rgb=({tl_r},{tl_g},{tl_b}) — pass 0's \
         paint was likely wiped by a later pass's Clear",
    );
    let Rgba([br_r, br_g, br_b, _]) = *f2.get_pixel(195, 195);
    assert!(
        br_r > br_g && br_r > br_b,
        "bottom-right corner pixel should be red-dominant (its painted \
         fill 0.7/0.3/0.2), got rgb=({br_r},{br_g},{br_b})",
    );
}

/// With `damage_rect` and a multi-rect region the overlay strokes *each* damage
/// rect: red in both corners, none in the centre.
#[test]
fn corner_pair_overlay_strokes_each_rect() {
    let mut h = Harness::new();
    let size = UVec2::new(200, 200);

    let _f1 = h.size(size).frame(corner_pair_scene("a", "a")).image;
    let f2 = h
        .size(size)
        .overlay(DebugOverlayConfig {
            damage_rect: true,
            ..Default::default()
        })
        .frame(corner_pair_scene("b", "b"))
        .image;
    let _kept = KeptOnFailure::new("damage_corner_pair_overlay_strokes_each_rect", &f2);

    // Each 20 px corner block is its own damage rect; the top-left outline hangs a
    // pixel off the surface.
    assert_outlines_exactly(
        &f2,
        &[PxRect::new(0, 0, 20, 20), PxRect::new(180, 180, 20, 20)],
    );
}

/// Regression for the blinking-caret bug: a damage rect thinner than twice the
/// overlay gap (a 2px sliver) must still get an outline; the overlay *outsets* the
/// rect, and an inset would collapse it.
#[test]
fn damage_rect_overlay_outlines_thin_sliver() {
    let mut h = Harness::new();
    let size = UVec2::new(120, 80);
    let sliver = |on: bool| {
        move |ui: &mut palantir::Ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: RgbaF32::srgb(0.12, 0.12, 0.15).into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("sliver")
                        .position(Vec2::new(60.0, 20.0))
                        .size((Sizing::fixed(2.0), Sizing::fixed(40.0)))
                        .background(Background {
                            fill: if on {
                                RgbaF32::srgb(0.9, 0.5, 0.2)
                            } else {
                                RgbaF32::srgb(0.2, 0.3, 0.7)
                            }
                            .into(),
                            ..Default::default()
                        })
                        .show(ui);
                });
        }
    };

    let _f1 = h.size(size).frame(sliver(false)).image;
    let f2 = h
        .size(size)
        .overlay(DebugOverlayConfig {
            damage_rect: true,
            ..Default::default()
        })
        .frame(sliver(true))
        .image;
    let _kept = KeptOnFailure::new("damage_rect_overlay_outlines_thin_sliver", &f2);

    assert_outlines_exactly(&f2, &[PxRect::new(60, 20, 2, 40)]);
}
