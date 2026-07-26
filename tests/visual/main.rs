//! Visual regression suite: drives `Ui` headlessly through wgpu, reads
//! the rendered texture into an `RgbaImage`, and compares against
//! committed golden PNGs in `tests/visual/golden/`. Missing goldens
//! are auto-created on first run; failures dump artifacts under
//! `tests/visual/output/<name>/`. See `visual-testing.md` next door.
//!
//! Layout: harness/diff/golden are the infrastructure; `fixtures/`
//! holds the actual UI scenes grouped by topic. Add new fixtures
//! there.

mod diff;
mod fixtures;
mod golden;
mod harness;

use glam::UVec2;
use image::Rgba;
use palantir::{Color, WindowConfig, WindowToken};

use crate::harness::Harness;

/// Smoke test of the harness: an empty scene reads back as the clear colour,
/// and a replayed record pass reproduces it pixel-for-pixel.
#[test]
fn readback_returns_clear_color_for_empty_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(16, 16);
    let (sr, sg, sb) = (0.5, 0.25, 0.75);
    let clear = Color::rgb(sr, sg, sb);
    let scene = |ui: &mut palantir::Ui| {
        // Vetoing a close the offscreen host never requests is a no-op, not an
        // error — unlike opening a window, which it cannot service at all.
        ui.keep_open();
        ui.request_relayout();
    };
    let img = h.render(size, 1.0, clear, scene);

    h.host.ui().request_repaint();
    let replayed = h.render(size, 1.0, clear, scene);
    assert_eq!(replayed, img);
    assert_eq!(img.dimensions(), (size.x, size.y));

    // sRGB → linear (in `Color::rgb`) → sRGB (wgpu's sRGB target) round-trips
    // to the original 8-bit sRGB values; ±2 covers rounding inside the pipeline.
    let expected = Rgba([
        (sr * 255.0).round() as u8,
        (sg * 255.0).round() as u8,
        (sb * 255.0).round() as u8,
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

/// The offscreen host has no window lifecycle, so a recorded open is a caller
/// error rather than a silently dropped request.
#[test]
#[should_panic(expected = "Ui::open_window(WindowToken(1))")]
fn opening_a_window_offscreen_panics() {
    let mut h = Harness::new();
    h.render(UVec2::new(16, 16), 1.0, Color::BLACK, |ui| {
        ui.open_window(WindowToken(1), WindowConfig::new("unservable"));
    });
}

/// Closing is denied on the same grounds as opening.
#[test]
#[should_panic(expected = "Ui::close_window(WindowToken(2))")]
fn closing_a_window_offscreen_panics() {
    let mut h = Harness::new();
    h.render(UVec2::new(16, 16), 1.0, Color::BLACK, |ui| {
        ui.close_window(WindowToken(2));
    });
}
