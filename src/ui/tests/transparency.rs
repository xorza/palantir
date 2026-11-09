//! Reduced transparency, as the system asks or a preference picks: every pane paints solid.

use glam::UVec2;

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::ui::Ui;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

const SURFACE: UVec2 = UVec2::new(200, 100);

/// A frosted panel's material captures what is under it under full transparency. The system's ask
/// records the next frame again with the material over the ground, which captures nothing, a pick
/// of full over that ask brings the capture back, and a pick of reduced over a system that asks
/// nothing takes it away again.
#[test]
fn reduced_transparency_records_every_pane_solid() {
    let id = WidgetId::from_hash("pane");
    let pane = Chrome::fill(RgbaF32::hex(0x204060).with_alpha8(184))
        .with_material(Material::frosted(20.0));
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .id(id)
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
            .chrome(pane.clone())
            .show(ui, |_| {});
    };
    let filtered = |h: &Harness| {
        let node = h.node_of(id).expect("the pane recorded").node;
        let row = h.ui().tree(Layer::Main).chrome(node).expect("a chrome row");
        row.material.is_some_and(LoweredMaterial::captures)
    };
    let mut h = Harness::mono(SURFACE);
    h.prime(2, record);
    assert!(filtered(&h), "full transparency");
    assert!(!h.ui().is_transparency_reduced());

    h.set_system_transparency_reduced(true);
    h.frame(record);
    assert!(h.ui().is_transparency_reduced());
    assert!(h.ui().system_transparency_reduced());
    assert!(!filtered(&h), "the system's ask");

    h.ui_mut().set_preferences(UiPreferences {
        transparency: Preference::Fixed(Reduction::Full),
        ..UiPreferences::default()
    });
    h.frame(record);
    assert!(!h.ui().is_transparency_reduced());
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

/// While transparency is reduced a material reads the window's ground, the clear, in place of what
/// is painted under it, and runs no blur: over a red square, a blurred pane under a quarter-white
/// fill paints the bytes the same pane paints at full transparency over nothing, where it reads the
/// clear. One that asks for nothing then paints its fill over the red square, as a plain
/// translucent rect does. The square stops short of the window's edge, so it folds into no clear.
#[test]
fn a_material_reads_the_ground_while_transparency_is_reduced() {
    let gpu = headless_test_gpu();
    let fill = RgbaF32::WHITE.with_alpha(0.25);
    let blur = Material::blur(4.0);
    let paint = |reduced: bool, under: bool, chrome: Chrome| {
        let mut h = Harness::builder(UVec2::new(32, 16))
            .mono()
            .depth(Depth::gpu(gpu.handles()))
            .build();
        h.set_system_transparency_reduced(reduced);
        h.prime(2, |ui| {
            if under {
                ui.add_shape(
                    Shape::rect(RgbaF32::hex(0xff0000)).at(Rect::new(0.0, 0.0, 24.0, 16.0)),
                );
            }
            ui.add_shape(Shape::rect(chrome.clone()).at(Rect::new(4.0, 4.0, 16.0, 8.0)));
        });
        (h.read_back().get_pixel(12, 8).0, h.backdrop_blurs())
    };
    let (ground, blurs) = paint(true, true, Chrome::fill(fill).with_material(blur));
    assert_eq!(blurs, 0, "the ground needs no blur");
    let (over_nothing, _) = paint(false, false, Chrome::fill(fill).with_material(blur));
    assert_eq!(ground, over_nothing);
    let (red, _) = paint(false, true, Chrome::fill(RgbaF32::TRANSPARENT));
    assert_ne!(ground, red, "the ground is no red");
    let dim = blur.with_reduced_transparency(ReducedBackdrop::Nothing);
    let (kept, _) = paint(true, true, Chrome::fill(fill).with_material(dim));
    let (plain, _) = paint(false, true, Chrome::fill(fill));
    assert_eq!(kept, plain);
}
