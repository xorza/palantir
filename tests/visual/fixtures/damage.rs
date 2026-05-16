//! DamageEngine visualization. Renders a static scene twice into the same
//! Harness (so `DamageEngine.prev` carries between frames). The second
//! render flips `DebugOverlayConfig::dim_undamaged` on and uses a
//! striking magenta clear: pixels outside the damage scissor stay
//! magenta, pixels inside flash the freshly-painted content. With
//! `SAVE_DAMAGE_PNGS=1` the second frame is written under
//! `tests/visual/output/damage/<name>.png` for inspection.

use std::path::Path;

use glam::{UVec2, Vec2};
use image::{Rgba, RgbaImage};
use palantir::{Background, Button, Color, Configure, DebugOverlayConfig, Frame, Panel, Sizing};

use crate::fixtures::DARK_BG;
use crate::harness::Harness;

/// Bright magenta — picked so non-painted pixels in the damage
/// visualization image stand out against any plausible UI palette.
const VIS_CLEAR: Color = Color::rgb(1.0, 0.0, 1.0);

fn save_debug(name: &str, img: &RgbaImage) {
    if std::env::var_os("SAVE_DAMAGE_PNGS").is_none_or(|v| v.is_empty()) {
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual/output/damage");
    std::fs::create_dir_all(&dir).expect("mkdir output/damage");
    let path = dir.join(format!("{name}.png"));
    img.save(&path).expect("save damage png");
    eprintln!("damage vis: wrote {}", path.display());
}

fn count_pixels(img: &RgbaImage, predicate: impl Fn(u8, u8, u8) -> bool) -> u32 {
    img.pixels()
        .filter(|p| {
            let Rgba([r, g, b, _]) = **p;
            predicate(r, g, b)
        })
        .count() as u32
}

// sRGB tolerances cover round-trip + AA fringes.
fn is_magenta(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b > 240
}
fn is_red(r: u8, g: u8, b: u8) -> bool {
    r > 240 && g < 16 && b < 16
}

fn button_scene(
    id_salt: &'static str,
    label: &'static str,
) -> impl FnMut(&mut palantir::UiCore) + Copy {
    move |ui: &mut palantir::UiCore| {
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .gap(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: Color::rgb(0.15, 0.15, 0.18).into(),
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
) -> impl FnMut(&mut palantir::UiCore) + Copy {
    move |ui: &mut palantir::UiCore| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: Color::rgb(0.15, 0.15, 0.18).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                Frame::new()
                    .id_salt(("tl", tl_label))
                    .position(Vec2::new(0.0, 0.0))
                    .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.7, 0.4).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt(("br", br_label))
                    .position(Vec2::new(180.0, 180.0))
                    .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.7, 0.3, 0.2).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    }
}

/// Two identical frames of a tiny static scene. After frame 1 seeds
/// `DamageEngine.prev`, frame 2's diff is empty and the renderer takes the
/// `Damage::Skip` path — no clear, no draw, the swapchain still
/// holds frame 1's pixels. Magenta is the *clear* color that never
/// ran, so a clean Skip means **zero magenta pixels** in the readback.
/// Any magenta ⇒ Skip didn't fire and we re-cleared.
#[test]
fn static_scene_repeats_clean() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);
    let scene = button_scene("hi", "hello");

    let _f1 = h.render(size, 1.0, DARK_BG, scene);
    let f2 = h.render_with_overlay(
        DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        },
        size,
        1.0,
        VIS_CLEAR,
        scene,
    );
    save_debug("static_scene_repeats_clean", &f2);

    let painted = count_pixels(&f2, |r, g, b| !is_magenta(r, g, b));
    let total = size.x * size.y;
    assert_eq!(
        painted, total,
        "static frame should hit the Skip path: backbuffer holds frame 1 \
         pixels and no magenta clear runs. Got {painted}/{total} non-magenta \
         pixels — Skip path didn't fire."
    );
}

/// One small thing actually changes between frames: button label flips
/// from "a" to "b". Pins that damage fires at all — `painted > 0` means
/// the renderer entered a paint pass and not the Skip path. On this
/// 160×96 fixture the damage rect crosses the 50%-coverage heuristic
/// and escalates to Full, so the captured frame is mostly painted —
/// don't pin an upper bound here.
#[test]
fn single_button_change_repaints_something() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);

    let _f1 = h.render(size, 1.0, DARK_BG, button_scene("b", "a"));
    let f2 = h.render_with_overlay(
        DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        },
        size,
        1.0,
        VIS_CLEAR,
        button_scene("b", "b"),
    );
    save_debug("single_button_change_repaints_something", &f2);

    let painted = count_pixels(&f2, |r, g, b| !is_magenta(r, g, b));
    let total = size.x * size.y;
    assert!(
        painted > 0,
        "button changed — damage should have painted something; got 0/{total}",
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

    let _f1 = h.render(size, 1.0, DARK_BG, button_scene("c", "a"));
    let f2 = h.render_with_overlay(
        DebugOverlayConfig {
            damage_rect: true,
            ..Default::default()
        },
        size,
        1.0,
        DARK_BG,
        button_scene("c", "b"),
    );
    save_debug("damage_rect_overlay_strokes_dirty_region", &f2);

    let red = count_pixels(&f2, is_red);
    let total = size.x * size.y;
    assert!(
        red > 0,
        "expected red overlay stroke pixels on the surface; \
         got {red}/{total} — post-copy overlay pass didn't reach the swapchain.",
    );
    // Sanity upper bound: the overlay is a 2px stroke around the
    // damage rect, never the whole surface.
    assert!(
        red < total / 4,
        "red pixel count {red}/{total} suggests the overlay flooded \
         the surface — should be a thin stroke around the dirty rect.",
    );
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

    let f1 = h.render(size, 1.0, DARK_BG, corner_pair_scene("a", "a"));
    let f2 = h.render_with_overlay(
        DebugOverlayConfig {
            dim_undamaged: true,
            ..Default::default()
        },
        size,
        1.0,
        VIS_CLEAR,
        corner_pair_scene("b", "b"),
    );
    save_debug("corner_pair_change_keeps_center_unpainted", &f2);

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
            f1_lum += r1 as u64 + g1 as u64 + b1 as u64;
            f2_lum += r2 as u64 + g2 as u64 + b2 as u64;
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

    let _f1 = h.render(size, 1.0, DARK_BG, corner_pair_scene("a", "a"));
    let f2 = h.render_with_overlay(
        DebugOverlayConfig {
            damage_rect: true,
            ..Default::default()
        },
        size,
        1.0,
        DARK_BG,
        corner_pair_scene("b", "b"),
    );
    save_debug("corner_pair_overlay_strokes_each_rect", &f2);

    let count_red_in = |x_range: std::ops::Range<u32>, y_range: std::ops::Range<u32>| {
        let mut n = 0u32;
        for y in y_range {
            for x in x_range.clone() {
                let Rgba([r, g, b, _]) = *f2.get_pixel(x, y);
                if is_red(r, g, b) {
                    n += 1;
                }
            }
        }
        n
    };
    let tl_red = count_red_in(0..40, 0..40);
    let br_red = count_red_in(160..200, 160..200);
    let centre_red = count_red_in(50..150, 50..150);
    assert!(tl_red > 0, "top-left corner must be outlined");
    assert!(br_red > 0, "bottom-right corner must be outlined");
    assert_eq!(
        centre_red, 0,
        "centre 100×100 must be free of overlay strokes (got {centre_red}) — \
         overlay should outline each damage rect, not their union",
    );
}
