//! DamageEngine visualization: a static scene rendered twice into one Harness (so
//! `DamageEngine.prev` carries over), the second with
//! `DebugOverlayConfig::dim_undamaged`. Magenta is the clear colour of **both**
//! frames, so a pixel reads magenta only if the scene left it uncovered. A failing
//! test writes its second frame under `tests/visual/output/damage_<name>/`.

use glam::{UVec2, Vec2};
use palantir::golden::image::{Rgba, RgbaImage};
use palantir::{
    Background, Block, Button, Configure, DebugOverlayConfig, FramePaint, Panel, Rect, RgbaF32,
    Sizing, Ui,
};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px};
use crate::goldens::{KeptOnFailure, assert_same, crop};
use crate::harness::Harness;

const VIS_CLEAR: RgbaF32 = RgbaF32::srgb(1.0, 0.0, 1.0);

const DIM_UNDAMAGED: DebugOverlayConfig = DebugOverlayConfig {
    dim_undamaged: true,
    damage_rect: false,
    frame_stats: false,
};

const DAMAGE_RECT: DebugOverlayConfig = DebugOverlayConfig {
    dim_undamaged: false,
    damage_rect: true,
    frame_stats: false,
};

const fn is_magenta(Rgba([r, g, b, _]): Rgba<u8>) -> bool {
    r > 240 && g < 16 && b > 240
}

const fn is_red(Rgba([r, g, b, _]): Rgba<u8>) -> bool {
    r > 240 && g < 16 && b < 16
}

/// Pixels of `img` that read as [`VIS_CLEAR`], which no scene paints.
fn magenta_pixels(img: &RgbaImage) -> usize {
    img.pixels().filter(|p| is_magenta(**p)).count()
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
        let (x, y) = (x as i32, y as i32);
        let expected = damage.iter().any(|rect| rect.outline_covers(x, y));
        assert_eq!(
            is_red(*p),
            expected,
            "pixel ({x}, {y}) {:?}: outline of {damage:?} expected {expected}",
            p.0,
        );
    }
}

/// The rect the red pixels bracket, for a damage rect the fixture cannot derive.
fn outlined_rect(img: &RgbaImage) -> PxRect {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y, p) in img.enumerate_pixels() {
        if is_red(*p) {
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

fn button_scene(id_salt: &'static str, label: &'static str) -> impl FnMut(&mut Ui) + Copy {
    move |ui: &mut Ui| {
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .gap(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background::fill(RgbaF32::srgb(0.15, 0.15, 0.18)))
            .show(ui, |ui| {
                Button::new().id_salt(id_salt).label(label).show(ui);
            });
    }
}

const TOP_LEFT: RgbaF32 = RgbaF32::srgb(0.2, 0.7, 0.4);
const BOTTOM_RIGHT: RgbaF32 = RgbaF32::srgb(0.7, 0.3, 0.2);

/// Two 20 px blocks in opposite corners, whose ids change with `salt`.
fn corner_pair_scene(salt: &'static str) -> impl FnMut(&mut Ui) + Copy {
    move |ui: &mut Ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background::fill(RgbaF32::srgb(0.15, 0.15, 0.18)))
            .show(ui, |ui| {
                Block::new()
                    .id_salt(("tl", salt))
                    .position(Vec2::new(0.0, 0.0))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .background(Background::fill(TOP_LEFT))
                    .show(ui);
                Block::new()
                    .id_salt(("br", salt))
                    .position(Vec2::new(180.0, 180.0))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .background(Background::fill(BOTTOM_RIGHT))
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
    let repeat = h.overlay(DIM_UNDAMAGED).frame(scene);
    assert_eq!(repeat.paint, FramePaint::Skip);
    let f2 = repeat.image;
    let _kept = KeptOnFailure::new("damage_static_scene_repeats_clean", &f2);

    assert_eq!(
        magenta_pixels(&f2),
        0,
        "the scene covers the surface, so no pixel may read as the clear colour",
    );
    assert_same("damage_static_repeat", &f2, &f1);
}

/// A button label flips from "a" to "b": the damage stays the button's rect, so the
/// frame repaints partially (target loaded, not cleared).
#[test]
fn single_button_change_repaints_partially() {
    let mut h = Harness::new();

    h.size(UVec2::new(160, 96))
        .clear(VIS_CLEAR)
        .frame(button_scene("b", "a"));
    let changed = h.overlay(DIM_UNDAMAGED).frame(button_scene("b", "b"));
    assert_eq!(changed.paint, FramePaint::Partial);
    let f2 = changed.image;
    let _kept = KeptOnFailure::new("damage_single_button_change_repaints_partially", &f2);

    assert_eq!(
        magenta_pixels(&f2),
        0,
        "a partial frame loads the target, so nothing reads as the clear colour",
    );
}

/// Smoke pin: `DebugOverlayConfig::damage_rect` puts red stroke pixels on the
/// swapchain; no other test exercises the post-copy pass.
#[test]
fn damage_rect_overlay_strokes_dirty_region() {
    let mut h = Harness::new();
    h.size(UVec2::new(160, 96)).frame(button_scene("c", "a"));
    let f2 = h.overlay(DAMAGE_RECT).frame(button_scene("c", "b")).image;
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
/// colour, and each corner reads its block's fill.
#[test]
fn corner_pair_change_keeps_center_unpainted() {
    let mut h = Harness::new();
    let f1 = h
        .size(UVec2::new(200, 200))
        .clear(VIS_CLEAR)
        .frame(corner_pair_scene("a"))
        .image;
    let f2 = h.overlay(DIM_UNDAMAGED).frame(corner_pair_scene("b")).image;
    let _kept = KeptOnFailure::new("damage_corner_pair_change_keeps_center_unpainted", &f2);

    let centre = Rect::new(50.0, 50.0, 100.0, 100.0);
    let (before, after) = (crop(&f1, centre), crop(&f2, centre));
    let lum = |img: &RgbaImage| -> u64 {
        img.pixels()
            .map(|Rgba([r, g, b, _])| u64::from(*r) + u64::from(*g) + u64::from(*b))
            .sum()
    };
    assert_eq!(
        magenta_pixels(&after),
        0,
        "the centre 100×100 must be free of magenta — dim_undamaged no longer fires \
         LoadOp::Clear, only a translucent dim pass",
    );
    assert!(
        lum(&after) < lum(&before),
        "the dim pre-pass darkens the centre",
    );

    // Each corner is a damaged pass of its own, repainted in full: a later
    // pass's clear would wipe the other's paint.
    for (x, y, fill) in [(5, 5, TOP_LEFT), (195, 195, BOTTOM_RIGHT)] {
        let fill = fill.to_srgba_u8();
        assert_px(
            f2.get_pixel(x, y).0,
            [fill.r, fill.g, fill.b, 255],
            SRGB_ROUND_TRIP,
            format_args!("({x}, {y}) is its corner block's fill"),
        );
    }
}

/// With `damage_rect` and a multi-rect region the overlay strokes *each* damage
/// rect: red in both corners, none in the centre.
#[test]
fn corner_pair_overlay_strokes_each_rect() {
    let mut h = Harness::new();
    h.size(UVec2::new(200, 200)).frame(corner_pair_scene("a"));
    let f2 = h.overlay(DAMAGE_RECT).frame(corner_pair_scene("b")).image;
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
    let sliver = |on: bool| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background::fill(RgbaF32::srgb(0.12, 0.12, 0.15)))
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("sliver")
                        .position(Vec2::new(60.0, 20.0))
                        .size((Sizing::fixed(2.0), Sizing::fixed(40.0)))
                        .background(Background::fill(if on {
                            RgbaF32::srgb(0.9, 0.5, 0.2)
                        } else {
                            RgbaF32::srgb(0.2, 0.3, 0.7)
                        }))
                        .show(ui);
                });
        }
    };

    h.size(UVec2::new(120, 80)).frame(sliver(false));
    let f2 = h.overlay(DAMAGE_RECT).frame(sliver(true)).image;
    let _kept = KeptOnFailure::new("damage_rect_overlay_outlines_thin_sliver", &f2);

    assert_outlines_exactly(&f2, &[PxRect::new(60, 20, 2, 40)]);
}
