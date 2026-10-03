//! DamageEngine visualization. Renders a static scene twice into the same
//! Harness (so `DamageEngine.prev` carries between frames). The second
//! render flips `DebugOverlayConfig::dim_undamaged` on.
//!
//! A striking magenta is the clear colour of **both** frames, so a pixel
//! reads magenta exactly when the scene covered it with nothing. One
//! colour for the pair on purpose: a clear colour that moves between
//! frames is itself a full repaint (`FrameBaseline`), and a full repaint
//! is the one classification these fixtures cannot observe anything
//! through. A failing test writes its second frame under
//! `tests/visual/output/damage_<name>/`.

use glam::{UVec2, Vec2};
use image::{Rgba, RgbaImage};
use palantir::{
    Background, Block, Button, Configure, DebugOverlayConfig, FramePaint, Panel, RgbaF32, Sizing,
};

use crate::goldens::{KeptOnFailure, assert_same};
use crate::harness::Harness;

/// Bright magenta — picked so non-painted pixels in the damage
/// visualization image stand out against any plausible UI palette.
const VIS_CLEAR: RgbaF32 = RgbaF32::srgb(1.0, 0.0, 1.0);

fn count_pixels(img: &RgbaImage, predicate: impl Fn(u8, u8, u8) -> bool) -> u32 {
    img.pixels()
        .filter(|p| {
            let Rgba([r, g, b, _]) = **p;
            predicate(r, g, b)
        })
        .count() as u32
}

// sRGB tolerances cover round-trip + AA fringes.
const fn is_magenta(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b > 240
}
const fn is_red(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b < 16
}

/// Physical px of the damage overlay's stroke at scale 1, drawn inside
/// its quad (`DAMAGE_OVERLAY_STROKE_WIDTH`).
const STROKE: i32 = 2;
/// How far the overlay quad sits outside the damage rect it brackets
/// (`DAMAGE_OVERLAY_GAP`).
const GAP: i32 = 1;

/// A damage rect in physical px, `max` exclusive.
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

    /// Whether the overlay outline bracketing this damage rect covers
    /// `(x, y)`: inside the quad grown by [`GAP`], and within [`STROKE`]
    /// of its edge.
    fn outline_covers(self, x: i32, y: i32) -> bool {
        let (x0, y0) = (self.min.0 - GAP, self.min.1 - GAP);
        let (x1, y1) = (self.max.0 + GAP, self.max.1 + GAP);
        let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
        let interior =
            (x0 + STROKE..x1 - STROKE).contains(&x) && (y0 + STROKE..y1 - STROKE).contains(&y);
        inside && !interior
    }
}

/// The red pixels of `img` are exactly the outlines of `damage`'s rects:
/// every pixel one covers is red, and no other pixel is.
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

/// The rect the red pixels of `img` bracket, read off their extent: the
/// overlay quad's bounds less the gap. For a damage rect the fixture
/// cannot derive, so [`assert_outlines_exactly`] can still check every
/// pixel of its outline.
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

/// Two identical frames of a tiny static scene. After frame 1 seeds
/// `DamageEngine.prev`, frame 2's diff is empty and the renderer gets no
/// plan at all — no clear, no draw, the target still holds frame 1's
/// pixels.
///
/// So frame 2 must read back as frame 1, bit for bit: the dim pass is
/// the one thing a partial frame would leave on those pixels, and it
/// runs on a plan this frame must not have. Zero magenta on top of that
/// says the scene covered every pixel, which is what makes the equality
/// worth asserting.
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
    // A repeat frame paints nothing, so nothing dims: the target still
    // holds frame 1.
    assert_same("damage_static_repeat", &f2, &f1);
}

/// One small thing actually changes between frames: button label flips
/// from "a" to "b". The damage stays the button's rect, under the
/// coverage that would escalate it, so the frame repaints partially: the
/// target is loaded rather than cleared, and no pixel reads as the clear
/// colour — the button repainted, the rest dimmed from frame 1.
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

/// Smoke-pin: with `DebugOverlayConfig::damage_rect = true`, the
/// post-copy overlay pass actually puts red stroke pixels on the
/// swapchain. Without coverage, "the F12 toggle does nothing" would
/// regress silently — no other test exercises the post-copy pass.
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

    // The button's label width decides the damage rect's size, so that is
    // read back; its corner is the panel's 12 px padding.
    let damage = outlined_rect(&f2);
    assert_eq!(damage.min, (12, 12), "the button sits at the padding");
    assert_outlines_exactly(&f2, &[damage]);
}

/// The motivating workload for multi-rect damage. Two tiny corner
/// frames change between frames; the rest of the canvas is static.
/// Under the old single-rect-union accumulator the union of the two
/// dirty corners would span the whole canvas (top-left + bottom-right
/// → bbox = entire surface) and trip the 50 %-coverage heuristic to
/// escalate `Damage::Full`. Under the multi-rect region the
/// corners stay disjoint (the LVGL merge rule rejects merging
/// far-apart rects), each scissored to its own pass.
///
/// `dim_undamaged` visualisation: the backend paints a full-viewport
/// 40%-translucent black quad over the backbuffer with `LoadOp::Load`
/// before any damage passes, then the partial passes paint their
/// rects at full brightness. The centre — outside both scissors —
/// therefore reads as frame 1's pixels darkened by ~40%, never as
/// the clear color (no `LoadOp::Clear` runs).
///
/// Three pinned regions:
/// 1. **Centre** must contain zero magenta pixels — a unioned-Full
///    repaint or the prior `LoadOp::Clear(VIS_CLEAR)` path would
///    flash the centre magenta.
/// 2. **Centre** must be measurably darker than the same pixels in
///    frame 1 — proves the dim pre-pass actually ran (otherwise
///    `LoadOp::Load` would just preserve frame 1's pixels verbatim).
/// 3. **Top-left** stays green-dominant; **bottom-right** stays
///    red-dominant — fresh paint inside each scissor wins over the
///    dim that briefly fell on them.
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

    // (1) Centre 100×100 region (50..150) lies outside both corner
    // scissors. Multi-rect damage keeps it that way; a unioned Full
    // repaint would flash magenta via PreClear / LoadOp::Clear.
    let centre_total: u32 = 100 * 100;
    let mut centre_magenta = 0u32;
    // (2) sum of brightness for f1 vs f2 over the centre — the dim
    // pre-pass should pull f2's centre noticeably darker than f1's.
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

    // (3) Sample one interior pixel of each corner. The foreground
    // colours have a unique dominant channel (TL green, BR red), so
    // the assertion is "dominant channel beats magenta's (255, 0, 255)
    // pattern" — robust under sRGB / gamma variation.
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

/// Pin: with `DebugOverlayConfig::damage_rect = true` and a multi-
/// rect region, the post-copy overlay pass strokes *each* damage
/// rect independently. Same scene as
/// `corner_pair_change_keeps_center_unpainted`; assert red overlay
/// pixels appear in *both* corner regions and not in the centre.
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

    // Each 20 px corner block is its own damage rect; their union would
    // be the whole surface. The top-left outline hangs a pixel off the
    // surface, which the exact check accounts for.
    assert_outlines_exactly(
        &f2,
        &[PxRect::new(0, 0, 20, 20), PxRect::new(180, 180, 20, 20)],
    );
}

/// Regression for the blinking-caret bug: a damage rect thinner than
/// twice the overlay gap (here a 2px sliver, a stand-in for the ~1px
/// text caret) must still get an outline. The overlay *outsets* the
/// damage rect; insetting it by more than its half-width would deflate
/// it to zero area and draw nothing.
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

    // An outset outline of the 2 × 40 sliver: a 4 × 42 quad, solid red
    // since its 2 px stroke meets itself across the width. An inset wider
    // than the rect would collapse it to nothing (the blinking-caret bug).
    assert_outlines_exactly(&f2, &[PxRect::new(60, 20, 2, 40)]);
}
