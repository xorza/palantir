//! A pane with nothing painted before it, whose backdrop is the clear.

use crate::gpu::backdrop::tests::{CLEAR, SIZE, assert_within_a_step, window};
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::Vec2;
use image::RgbaImage;

/// σ 2 reaches 8 px, so the pane's footprint spans x 8..56: a speck at the window's corner lies
/// outside it, and a veil over x 4..60 covers it without covering the window.
const PANE: Rect = Rect::new(16.0, 8.0, 32.0, 32.0);
const SPECK: Rect = Rect::new(0.0, 0.0, 2.0, 2.0);
const VEIL: Rect = Rect::new(4.0, 0.0, 56.0, 48.0);

/// What a scene paints before its pane, past its background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before {
    Nothing,
    /// A red speck outside the pane's footprint.
    Speck,
    /// [`CLEAR`] over the pane's footprint, so the pane captures the clear's bytes.
    Veil,
}

/// A frosted pane at [`PANE`], after a background of `background` over the window, which folds
/// into the clear, then `before`, and a square inside the pane when `under` is some.
fn scene(
    background: Option<RgbaF32>,
    before: Before,
    under: Option<RgbaF32>,
) -> impl FnMut(&mut Ui) {
    move |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("backdrop-clear")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if let Some(background) = background {
                    ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 64.0, 48.0)).fill(background));
                }
                match before {
                    Before::Nothing => {}
                    Before::Speck => {
                        ui.add_shape(Shape::rect(SPECK).fill(RgbaF32::srgb(1.0, 0.2, 0.1)));
                    }
                    Before::Veil => ui.add_shape(Shape::rect(VEIL).fill(CLEAR)),
                }
                if let Some(under) = under {
                    ui.add_shape(Shape::rect(Rect::new(24.0, 16.0, 8.0, 8.0)).fill(under));
                }
                let filter = BackdropFilter {
                    saturation: 1.6,
                    ..BackdropFilter::blur(2.0)
                };
                ui.add_shape(
                    Shape::rect(PANE)
                        .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.25))
                        .backdrop_filter(filter),
                );
            });
    }
}

/// What a fresh window that clears to `clear` paints of `scene`, and the blurs it ran.
fn paint(clear: RgbaF32, mut scene: impl FnMut(&mut Ui)) -> (RgbaImage, usize) {
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, SIZE, clear);
    window.frame(&mut scene);
    (window.read_back(), window.backdrop_blurs())
}

/// A pane with nothing painted under its footprint shows the clear without a capture or a blur,
/// as one whose capture holds only the clear's bytes shows it: the same bytes, as the clear it
/// shows is the one the target holds, rounded to its byte and to the blur's `f16`. Its edge's
/// coverage blends the same pane, so it matches too. A speck outside the footprint leaves it on
/// the clear. A background over the whole window folds into the clear, so a pane after it shows
/// the background the same way.
#[test]
fn a_pane_with_nothing_under_it_shows_the_clear() {
    for (clear, background, what) in [
        (CLEAR, None, "the window's clear"),
        (RgbaF32::BLACK, Some(CLEAR), "a folded background"),
    ] {
        let (captured, blurs) = paint(clear, scene(background, Before::Veil, None));
        assert_eq!(blurs, 1, "{what}: the veil");
        let clear_bytes = CLEAR.to_srgba8();
        for before in [Before::Nothing, Before::Speck] {
            let (flat, blurs) = paint(clear, scene(background, before, None));
            assert_eq!(blurs, 0, "{what}: {before:?}");
            for (x, y, pixel) in flat.enumerate_pixels() {
                let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if SPECK.contains(at) {
                    continue;
                }
                assert_eq!(
                    pixel,
                    captured.get_pixel(x, y),
                    "{what}: {before:?} ({x}, {y})"
                );
                if !PANE.contains(at) {
                    assert_eq!(
                        pixel.0,
                        [clear_bytes.r, clear_bytes.g, clear_bytes.b, 255],
                        "{what}: {before:?} ({x}, {y}) outside the pane"
                    );
                }
            }
        }
    }
}

/// On a kept window, a pane shows the clear while nothing lies under it, captures once a square
/// appears under it, and shows the clear again once the square leaves, freeing its result: each
/// frame matches a whole repaint of its scene within one 8-bit step.
#[test]
fn a_pane_shows_the_clear_or_a_capture_as_what_lies_under_it_changes() {
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, SIZE, CLEAR);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    for (under, what, want) in [
        (None, "first", (FramePaint::Full, 0)),
        (Some(red), "a square under", (FramePaint::Partial, 1)),
        (Some(blue), "a square changed", (FramePaint::Partial, 1)),
        (None, "the square gone", (FramePaint::Partial, 0)),
        (Some(red), "back", (FramePaint::Partial, 1)),
    ] {
        let painted = window
            .frame(&mut scene(None, Before::Nothing, under))
            .paint();
        assert_eq!((painted, window.backdrop_blurs()), want, "{what}");
        let (whole, _) = paint(CLEAR, scene(None, Before::Nothing, under));
        assert_within_a_step(&window.read_back(), &whole, what);
    }
}
