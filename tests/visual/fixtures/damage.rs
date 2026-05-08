//! Damage visualization. Renders a static scene twice into the same
//! Harness (so `Damage.prev` carries between frames). The second
//! render flips `debug_clear_on_damage` on and uses a striking
//! magenta clear: pixels outside the damage scissor stay magenta,
//! pixels inside flash the freshly-painted content. The PNG goes to
//! `tests/visual/output/damage/<name>.png` for inspection — the
//! tests assert nothing beyond "second-frame damage shouldn't repaint
//! the whole panel" so they're useful as a diagnostic without
//! coupling to specific damage rects.

use std::path::Path;

use glam::UVec2;
use image::{Rgba, RgbaImage};
use palantir::{Background, Button, Color, Configure, Panel, Sizing};

use crate::fixtures::DARK_BG;
use crate::harness::Harness;

/// Bright magenta — picked so non-painted pixels in the damage
/// visualization image stand out against any plausible UI palette.
const VIS_CLEAR: Color = Color::rgb(1.0, 0.0, 1.0);

fn save_debug(name: &str, img: &RgbaImage) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual/output/damage");
    std::fs::create_dir_all(&dir).expect("mkdir output/damage");
    let path = dir.join(format!("{name}.png"));
    img.save(&path).expect("save damage png");
    eprintln!("damage vis: wrote {}", path.display());
}

fn count_non_magenta(img: &RgbaImage) -> u32 {
    let mut n = 0u32;
    for p in img.pixels() {
        let Rgba([r, g, b, _]) = *p;
        // sRGB → magenta is (255, 0, 255). Tolerance covers round-trip.
        let is_magenta = r > 240 && g < 16 && b > 240;
        if !is_magenta {
            n += 1;
        }
    }
    n
}

/// Two identical frames of a tiny static scene. After frame 1 seeds
/// `Damage.prev`, frame 2's diff should yield empty damage — and so
/// the magenta-clear pass should produce an entirely magenta image.
/// Any non-magenta pixel ⇒ we re-painted something on a frame where
/// nothing changed.
#[test]
fn static_scene_repeats_clean() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);

    let scene = |ui: &mut palantir::Ui| {
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .gap(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: Color::rgb(0.15, 0.15, 0.18),
                ..Default::default()
            })
            .show(ui, |ui| {
                Button::new().id_salt("hi").label("hello").show(ui);
            });
    };

    // Frame 1: normal paint, seeds Damage.prev.
    let _f1 = h.render(size, 1.0, DARK_BG, scene);

    // Frame 2: same scene, but flash undamaged pixels magenta.
    palantir::support::internals::set_clear_on_damage(&mut h.backend, true);
    let f2 = h.render(size, 1.0, VIS_CLEAR, scene);
    palantir::support::internals::set_clear_on_damage(&mut h.backend, false);

    save_debug("static_scene_repeats_clean", &f2);

    let painted = count_non_magenta(&f2);
    let total = size.x * size.y;
    eprintln!("static-scene frame 2 painted {painted}/{total} pixels");
    // Stage 2 contract: a true static frame yields `DamagePaint::Skip`,
    // which short-circuits the render pass. The backbuffer carries
    // *frame 1*'s pixels into the swapchain copy — none of which are
    // magenta. So the readback should be entirely the rendered scene,
    // not the magenta clear color. The `count_non_magenta` check is
    // thus the reverse of intuition: every pixel non-magenta = Skip
    // path was taken, no clear ran.
    assert_eq!(
        painted, total,
        "static frame should hit the Skip path: backbuffer holds frame 1 \
         pixels and no magenta clear runs. Got {painted}/{total} non-magenta \
         pixels — Skip path didn't fire."
    );
}

/// One small thing actually changes between frames: button label flips
/// from "a" to "b". Damage *should* land on (roughly) the button rect
/// only — surrounding panel + padding stays magenta in the output.
#[test]
fn single_button_change_paints_button_only() {
    let mut h = Harness::new();
    let size = UVec2::new(160, 96);

    let frame_with = |label: &'static str| {
        move |ui: &mut palantir::Ui| {
            Panel::vstack()
                .auto_id()
                .padding(12.0)
                .gap(8.0)
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.15, 0.15, 0.18),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Button::new().id_salt("b").label(label).show(ui);
                });
        }
    };

    let _f1 = h.render(size, 1.0, DARK_BG, frame_with("a"));

    palantir::support::internals::set_clear_on_damage(&mut h.backend, true);
    let f2 = h.render(size, 1.0, VIS_CLEAR, frame_with("b"));
    palantir::support::internals::set_clear_on_damage(&mut h.backend, false);

    save_debug("single_button_change_paints_button_only", &f2);

    let painted = count_non_magenta(&f2);
    let total = size.x * size.y;
    eprintln!("single-change frame 2 painted {painted}/{total} pixels");
}
