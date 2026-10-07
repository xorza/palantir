//! Visual regression suite: drives `Ui` headlessly through wgpu, reads the texture back and compares against golden PNGs in `tests/visual/golden/`; failures dump artifacts under `tests/visual/output/<name>/`.
//!
//! Both directories are gitignored, so baselines are local. A fresh checkout has none: the first run writes each golden and fails it, so someone looks before it becomes the reference.
//!
//! `harness` raises the UI, `palantir::golden` compares, `fixtures/` holds the scenes. Name each new golden in `golden_name`.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

mod fixtures;
mod golden_name;
mod goldens;
mod harness;
/// The showcase's support module, compiled in so a golden renders a showcase page itself.
#[expect(
    dead_code,
    unused_macro_rules,
    reason = "the module is compiled whole, and only the part those pages call is used here"
)]
#[path = "../../examples/showcase/support.rs"]
mod support;

use glam::UVec2;
use palantir::{FramePaint, RgbaF32, WindowConfig, WindowToken};

use crate::harness::Harness;

/// Harness smoke test: an empty scene reads back as the clear colour, and a replayed record pass reproduces it pixel-for-pixel.
#[test]
fn readback_returns_clear_color_for_empty_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(16, 16);
    let (sr, sg, sb) = (0.5, 0.25, 0.75);
    let clear = RgbaF32::srgb(sr, sg, sb);
    let scene = |ui: &mut palantir::Ui| {
        // Vetoing a close the offscreen host never requests is a no-op; opening a window is not serviceable.
        ui.keep_open();
        ui.request_relayout();
    };
    let first = h.size(size).clear(clear).frame(scene);
    assert_eq!(first.paint, FramePaint::Full);
    let img = first.image;

    // Invalidated so the replay repaints; an unchanged scene would skip.
    h.host.invalidate_target_contents();
    let replayed = h.frame(scene);
    assert_eq!(replayed.paint, FramePaint::Full, "the replay repaints");
    goldens::assert_same("replay_empty_scene", &replayed.image, &img);
    assert_eq!(img.dimensions(), (size.x, size.y));

    // sRGB → linear (`RgbaF32::srgb`) → sRGB (wgpu target) round-trips to the original 8-bit values.
    let expected = [
        (sr * 255.0).round() as u8,
        (sg * 255.0).round() as u8,
        (sb * 255.0).round() as u8,
        255,
    ];
    for (x, y, p) in img.enumerate_pixels() {
        fixtures::assert_px(
            p.0,
            expected,
            fixtures::SRGB_ROUND_TRIP,
            format_args!("clear pixel ({x}, {y})"),
        );
    }
}

/// The offscreen host has no window lifecycle, so a recorded open is a caller error.
#[test]
#[should_panic(expected = "Ui::open_window(WindowToken(1))")]
fn opening_a_window_offscreen_panics() {
    let mut h = Harness::new();
    h.size(UVec2::new(16, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            ui.open_window(WindowToken(1), WindowConfig::new("unservable"));
        });
}

/// Closing is denied on the same grounds as opening.
#[test]
#[should_panic(expected = "Ui::close_window(WindowToken(2))")]
fn closing_a_window_offscreen_panics() {
    let mut h = Harness::new();
    h.size(UVec2::new(16, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            ui.close_window(WindowToken(2));
        });
}
