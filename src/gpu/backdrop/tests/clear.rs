//! A material with nothing painted before it, whose backdrop is the clear.

use crate::gpu::backdrop::tests::{CLEAR, Reference, SIZE, assert_within_a_step, window};
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::paint::Paint;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::Vec2;

/// σ 2 reaches 8 px, so the backdrop's footprint spans x 8..56: a speck at the window's corner lies
/// outside it, and a veil over x 4..60 covers it without covering the window.
const BACKDROP: Rect = Rect::new(16.0, 8.0, 32.0, 32.0);
const SPECK: Rect = Rect::new(0.0, 0.0, 2.0, 2.0);
const VEIL: Rect = Rect::new(4.0, 0.0, 56.0, 48.0);

/// What a scene paints before its backdrop, past its background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before {
    Nothing,
    /// A red speck outside the backdrop's footprint.
    Speck,
    /// [`CLEAR`] over the backdrop's footprint, so the backdrop captures the clear's bytes.
    Veil,
}

/// A frosted backdrop at [`BACKDROP`], after a background of `background` over the window, which
/// folds into the clear, then `before`, and a square inside the backdrop when `under` is some.
fn scene(
    background: Option<RgbaF32>,
    before: Before,
    under: Option<RgbaF32>,
) -> impl FnMut(&mut Ui) {
    move |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("backdrop-clear")
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                if let Some(background) = background {
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 64.0, 48.0)),
                        background,
                    );
                }
                match before {
                    Before::Nothing => {}
                    Before::Speck => {
                        ui.add_shape(Shape::rect().at(SPECK), RgbaF32::srgb(1.0, 0.2, 0.1));
                    }
                    Before::Veil => {
                        ui.add_shape(Shape::rect().at(VEIL), CLEAR);
                    }
                }
                if let Some(under) = under {
                    ui.add_shape(Shape::rect().at(Rect::new(24.0, 16.0, 8.0, 8.0)), under);
                }
                let filter = Material::new(
                    EffectName::FROSTED,
                    FrostedParams {
                        saturation: 1.6,
                        noise: 0.0,
                    },
                )
                .with_backdrop_blur(2.0);
                ui.add_shape(
                    Shape::rect().at(BACKDROP),
                    Paint::from(RgbaF32::new(1.0, 1.0, 1.0, 0.25)).with_material(filter),
                );
            });
    }
}

/// A backdrop with nothing painted under its footprint shows the clear without a capture or a blur,
/// as one whose capture holds only the clear's bytes shows it: the same bytes, as the clear it
/// shows is the one the target holds, rounded to its byte and to the blur's `f16`. Its edge's
/// coverage blends the same backdrop, so it matches too. A speck outside the footprint leaves it on
/// the clear. A background over the whole window folds into the clear, so a backdrop after it shows
/// the background the same way.
#[test]
fn a_backdrop_with_nothing_under_it_shows_the_clear() {
    for (clear, background, what) in [
        (CLEAR, None, "the window's clear"),
        (RgbaF32::BLACK, Some(CLEAR), "a folded background"),
    ] {
        let gpu = headless_test_gpu();
        let mut reference = Reference::new(&gpu, SIZE, clear);
        let captured = reference.paint(scene(background, Before::Veil, None));
        assert_eq!(reference.blurs(), 1, "{what}: the veil");
        let clear_bytes = CLEAR.to_srgba8();
        for before in [Before::Nothing, Before::Speck] {
            let flat = reference.paint(scene(background, before, None));
            assert_eq!(reference.blurs(), 0, "{what}: {before:?}");
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
                if !BACKDROP.contains(at) {
                    assert_eq!(
                        pixel.0,
                        [clear_bytes.r, clear_bytes.g, clear_bytes.b, 255],
                        "{what}: {before:?} ({x}, {y}) outside the backdrop"
                    );
                }
            }
        }
    }
}

/// On a kept window, a backdrop shows the clear while nothing lies under it, captures once a square
/// appears under it, and shows the clear again once the square leaves, freeing its result: each
/// frame matches a whole repaint of its scene within one 8-bit step.
#[test]
fn a_backdrop_shows_the_clear_or_a_capture_as_what_lies_under_it_changes() {
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, SIZE, CLEAR);
    let mut reference = Reference::new(&gpu, SIZE, CLEAR);
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
        let whole = reference.paint(scene(None, Before::Nothing, under));
        assert_within_a_step(&window.read_back(), &whole, what);
    }
}
