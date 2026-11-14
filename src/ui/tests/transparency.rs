//! Reduced transparency, as the system asks or a preference picks: every material paints its
//! fallback, with no capture or blur.

use glam::UVec2;

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

const SURFACE: UVec2 = UVec2::new(200, 100);

/// A frosted panel's material captures what is under it under full transparency. The system's ask
/// records the next frame again with the material painting its fallback, which captures nothing,
/// a pick of full over that ask brings the capture back, and a pick of reduced over a system that
/// asks nothing takes it away again.
#[test]
fn reduced_transparency_records_every_material_by_its_fallback() {
    let id = WidgetId::from_hash("backdrop");
    let panel = RgbaF32::hex(0x204060);
    let backdrop = Paint::from(panel.with_alpha8(184))
        .with_material(Material::frosted(20.0).with_fallback(panel));
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .id(id)
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(), backdrop.clone());
            });
    };
    let filtered = |h: &Harness| {
        h.node_of(id).expect("the backdrop recorded");
        match h.ui().tree(Layer::Main).backdrops.as_slice() {
            [] => false,
            [_] => true,
            listed => panic!("one material, listed {} times", listed.len()),
        }
    };
    let mut h = Harness::mono(SURFACE);
    h.prime(2, record);
    assert!(filtered(&h), "full transparency");
    assert!(!h.ui().reductions().transparency.is_reduced());

    h.set_system_transparency_reduced(true);
    h.frame(record);
    assert!(h.ui().reductions().transparency.is_reduced());
    assert!(h.ui().system_reductions().transparency.is_reduced());
    assert!(!filtered(&h), "the system's ask");

    h.ui_mut().set_preferences(UiPreferences {
        transparency: Preference::Fixed(Reduction::Unreduced),
        ..UiPreferences::default()
    });
    h.frame(record);
    assert!(!h.ui().reductions().transparency.is_reduced());
    assert!(filtered(&h), "a pick of full over the system's ask");

    h.set_system_transparency_reduced(false);
    h.ui_mut().set_preferences(UiPreferences {
        transparency: Preference::Fixed(Reduction::Reduced),
        ..UiPreferences::default()
    });
    h.frame(record);
    assert_eq!(
        h.ui().preferences().transparency,
        Preference::Fixed(Reduction::Reduced)
    );
    assert!(!filtered(&h), "a pick of reduced");
}

/// While transparency is reduced a material's box draws as a plain box of its fallback, its fill
/// gone, and runs no blur: over a red square, a quarter-white box falling back to opaque blue paints
/// the bytes a plain blue rect does; one falling back to half black those of a plain half-black
/// rect; one falling back to a gradient those of a rect of that gradient; and one falling back to
/// nothing paints none, the red showing. The square stops short of the window's edge, so it folds
/// into no clear.
#[test]
fn a_material_draws_as_a_plain_box_of_its_fallback_while_transparency_is_reduced() {
    let gpu = headless_test_gpu();
    let fill = RgbaF32::WHITE.with_alpha(0.25);
    let paint = |reduced: bool, under: bool, draw: &dyn Fn(&mut Ui)| {
        let mut h = Harness::builder(UVec2::new(32, 16))
            .mono()
            .depth(HarnessDepth::gpu(gpu.handles()))
            .build();
        h.set_system_transparency_reduced(reduced);
        h.prime(2, |ui| {
            if under {
                ui.add_shape(
                    Shape::rect().at(Rect::new(0.0, 0.0, 24.0, 16.0)),
                    RgbaF32::hex(0xff0000),
                );
            }
            draw(ui);
        });
        (h.read_back().get_pixel(12, 8).0, h.backdrop_blurs())
    };
    let at = || Shape::rect().at(Rect::new(4.0, 4.0, 16.0, 8.0));
    let reduced = |fallback: Brush| {
        let worn = Paint::from(fill).with_material(Material::blur(4.0).with_fallback(fallback));
        paint(true, true, &|ui| {
            ui.add_shape(at(), worn.clone());
        })
    };
    let plain = |brush: Brush| {
        paint(false, true, &|ui| {
            ui.add_shape(at(), brush.clone());
        })
        .0
    };
    let blue: Brush = RgbaF32::new(0.0, 0.0, 1.0, 1.0).into();
    let (opaque, blurs) = reduced(blue.clone());
    assert_eq!(blurs, 0, "a fallback needs no blur");
    assert_eq!(opaque, plain(blue), "an opaque fallback");
    let half_black: Brush = RgbaF32::BLACK.with_alpha(0.5).into();
    assert_eq!(
        reduced(half_black.clone()).0,
        plain(half_black),
        "a translucent one"
    );
    let ramp: Brush = Gradient::horizontal([
        RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        RgbaF32::new(0.0, 0.0, 1.0, 1.0),
    ])
    .into();
    assert_eq!(reduced(ramp.clone()).0, plain(ramp), "a gradient");
    let red = plain(Brush::TRANSPARENT);
    assert_eq!(reduced(Brush::TRANSPARENT).0, red, "nothing");
    assert_ne!(opaque, red, "the fallback hides the red");
}
