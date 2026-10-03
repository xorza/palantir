//! Visual regression suite: drives `Ui` headlessly through wgpu, reads
//! the rendered texture into an `RgbaImage`, and compares against
//! stored golden PNGs in `tests/visual/golden/`. Failures dump artifacts
//! under `tests/visual/output/<name>/`.
//!
//! Both directories are gitignored, so the baseline is local rather than
//! something a diff reviews. A fresh checkout has no goldens: the first run
//! writes every one and fails it, so someone looks before they become the
//! reference.
//!
//! Layout: `harness` raises the UI, `palantir::golden` does the comparing,
//! and `fixtures/` holds the actual UI scenes grouped by topic. Add new
//! fixtures there.

mod fixtures;
mod goldens;
mod harness;
/// The showcase's support module, compiled into this suite so a golden can
/// render a showcase page itself rather than a copy that drifts from it.
/// Whole, so only the part those pages call is used here.
#[allow(dead_code)]
#[path = "../../examples/showcase/support.rs"]
mod support;

use glam::UVec2;
use palantir::{FramePaint, RgbaF32, WindowConfig, WindowToken};

use crate::harness::Harness;

/// Smoke test of the harness: an empty scene reads back as the clear colour,
/// and a replayed record pass reproduces it pixel-for-pixel.
#[test]
fn readback_returns_clear_color_for_empty_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(16, 16);
    let (sr, sg, sb) = (0.5, 0.25, 0.75);
    let clear = RgbaF32::srgb(sr, sg, sb);
    let scene = |ui: &mut palantir::Ui| {
        // Vetoing a close the offscreen host never requests is a no-op, not an
        // error — unlike opening a window, which it cannot service at all.
        ui.keep_open();
        ui.request_relayout();
    };
    let first = h.size(size).clear(clear).frame(scene);
    assert_eq!(first.paint, FramePaint::Full);
    let img = first.image;

    // Invalidated, so the replay repaints: an unchanged scene would
    // otherwise skip and present a copy of the backbuffer.
    h.host.invalidate_target_contents();
    let replayed = h.frame(scene);
    assert_eq!(replayed.paint, FramePaint::Full, "the replay repaints");
    goldens::assert_same("replay_empty_scene", &replayed.image, &img);
    assert_eq!(img.dimensions(), (size.x, size.y));

    // sRGB → linear (in `RgbaF32::srgb`) → sRGB (wgpu's sRGB target) round-trips
    // to the original 8-bit sRGB values.
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

/// The offscreen host has no window lifecycle, so a recorded open is a caller
/// error rather than a silently dropped request.
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
