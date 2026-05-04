//! Visual regression suite: drives `Ui` headlessly through wgpu, reads
//! the rendered texture into an `RgbaImage`, and compares against
//! committed golden PNGs in `tests/visual/golden/`. Missing goldens
//! are auto-created on first run; failures dump artifacts under
//! `tests/visual/output/<name>/`. See `docs/visual-testing.md`.

mod diff;
mod golden;
mod harness;

use glam::UVec2;
use image::Rgba;
use palantir::{Button, Color, Configure, Sizing};

use crate::diff::Tolerance;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

#[test]
fn readback_returns_clear_color_for_empty_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(16, 16);
    let clear = Color::rgb(0.5, 0.25, 0.75);
    let img = h.render(size, 1.0, clear, |_| {});
    assert_eq!(img.dimensions(), (size.x, size.y));

    // sRGB-encoded clear color, allowing ±1 for rounding through the
    // linear↔sRGB pipeline.
    let expected = Rgba([
        (clear.r.powf(1.0 / 2.2) * 255.0).round() as u8,
        (clear.g.powf(1.0 / 2.2) * 255.0).round() as u8,
        (clear.b.powf(1.0 / 2.2) * 255.0).round() as u8,
        255,
    ]);
    for p in img.pixels() {
        for c in 0..4 {
            assert!(
                p.0[c].abs_diff(expected.0[c]) <= 2,
                "pixel {p:?} far from expected clear {expected:?}",
            );
        }
    }
}

#[test]
fn button_hello_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(
        UVec2::new(256, 96),
        1.0,
        Color::rgb(0.08, 0.08, 0.10),
        |ui| {
            Button::new()
                .label("hello")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
        },
    );
    assert_matches_golden("button_hello", &img, Tolerance::default());
}
