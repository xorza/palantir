use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::color_swatch::ColorSwatch;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::UVec2;

/// A chip is a `swatch_size` square from the theme or the bundle `style` names. Opaque, it records one shape, the colour. Translucent, the checker goes behind first: the light fill, then a dark cell on every other square of the `checker_cell` grid (24 / 6 = 4 cells a side, two dark per row over four rows).
#[test]
fn a_chip_is_a_themed_square_with_a_checker_behind_translucency() {
    let id = WidgetId::from_hash("chip");
    let styled = ColorPickerTheme {
        swatch_size: 24.0,
        checker_cell: 6.0,
        ..ColorPickerTheme::default()
    };
    let stock = ColorPickerTheme::default().swatch_size;
    assert_ne!(stock, 24.0, "premise: the style moves the side");
    for (style, color, side, shapes) in [
        (None, RgbaF32::srgb(0.2, 0.4, 0.6), stock, 1),
        (Some(&styled), RgbaF32::srgb(0.2, 0.4, 0.6), 24.0, 1),
        (
            Some(&styled),
            RgbaF32::srgba(0.2, 0.4, 0.6, 0.5),
            24.0,
            1 + 4 * 2 + 1,
        ),
    ] {
        let mut h = UiHarness::new(UVec2::new(100, 100));
        let node = h.frame_value(|ui| ColorSwatch::new(color).id(id).style(style).show(ui).node());
        let rect = h.arranged(id);
        assert_eq!((rect.size.w, rect.size.h), (side, side), "{color:?}");
        assert_eq!(
            h.ui.tree(Layer::Main).shapes_of(node).count(),
            shapes,
            "{color:?}: shapes recorded",
        );
    }
}

/// A chip senses a click and writes nothing — the caller reads it.
#[test]
fn a_chip_reports_its_click() {
    let id = WidgetId::from_hash("chip-click");
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let record = |h: &mut UiHarness| {
        h.frame_value(|ui| {
            ColorSwatch::new(RgbaF32::WHITE)
                .id(id)
                .show(ui)
                .left
                .clicked()
        })
    };
    assert!(!record(&mut h));
    h.click_on(id);
    assert!(record(&mut h), "the click lands");
    assert!(!record(&mut h), "and is one-shot");
}
