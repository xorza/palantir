//! A material's fill and border: drawn in the material's one draw, over the whole box.

use crate::gpu::backdrop::tests::{CLEAR, SIZE, assert_within_a_step, encode, window};
use crate::gpu::test_gpu::headless_test_gpu;
use crate::internals::paint_capture::PaintCall;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

const BOX: Rect = Rect::new(8.0, 8.0, 48.0, 32.0);

/// A canvas over the window that paints `paint` over [`BOX`].
fn boxed(paint: Paint) -> impl FnMut(&mut Ui) {
    move |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("backdrop-fill")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect().at(BOX), paint.clone());
            });
    }
}

/// An opaque gradient fill hides the material under it whole, so a material's box shows the
/// gradient a plain box of it shows, within the dither's step, and in one draw: the material's,
/// with no quad for the fill above it.
#[test]
fn a_gradient_fill_draws_in_the_material_draw() {
    let gpu = headless_test_gpu();
    let gradient = LinearGradient::two_stop(
        0.0,
        RgbaF32::srgb(1.0, 0.1, 0.0),
        RgbaF32::srgb(0.0, 0.2, 1.0),
    );
    let mut plain = window(&gpu, SIZE, CLEAR);
    plain.frame(boxed(Paint::from(gradient.clone())));
    let mut material = window(&gpu, SIZE, CLEAR);
    material.frame(boxed(
        Paint::from(gradient).with_material(Material::blur(2.0)),
    ));
    assert_within_a_step(
        &material.read_back(),
        &plain.read_back(),
        "a material's gradient fill",
    );
    let calls = material.encode_paint().calls;
    let [PaintCall::Effect(p)] = calls.as_slice() else {
        panic!("one draw, the material's");
    };
    assert!(p.stage.is_some(), "the draw wears the material's stage");
}

/// A translucent border lies over the material's opaque fill, as CSS's border-box background
/// lies under it: at the band's middle, half red over green is linear (0.5, 0.5, 0), which encodes
/// to the byte 188 on each, where a fill inside the border alone would show the clear there.
#[test]
fn a_border_lies_over_the_material() {
    let gpu = headless_test_gpu();
    let paint = Paint::from(RgbaF32::new(0.0, 1.0, 0.0, 1.0))
        .with_border(Border::new(RgbaF32::new(1.0, 0.0, 0.0, 0.5), 4.0))
        .with_material(Material::blur(2.0));
    let mut h = window(&gpu, SIZE, CLEAR);
    h.frame(boxed(paint));
    // The band runs 4 px in from the box's left edge, at x 8.
    let pixel = h.read_back().get_pixel(10, 24).0;
    let half = encode(0.5);
    for (c, want) in [half, half, 0.0].into_iter().enumerate() {
        assert!(
            (f64::from(pixel[c]) - want).abs() <= 1.0,
            "channel {c} is {}, {want:.1} wanted",
            pixel[c]
        );
    }
    let calls = h.encode_paint().calls;
    let [PaintCall::Effect(p)] = calls.as_slice() else {
        panic!("one draw, the material's");
    };
    assert!(p.stage.is_some(), "the draw wears the material's stage");
}
