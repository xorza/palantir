use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::color_button::ColorButton;
use crate::widgets::configure::Configure;
use glam::{UVec2, Vec2};

fn harness() -> UiHarness {
    UiHarness::with_text(UVec2::new(420, 560))
}

/// How many nodes the popup layer holds — zero while the chip is closed.
fn panel_nodes(h: &UiHarness) -> usize {
    h.ui.layout(Layer::Popup).rect.len()
}

/// The chip opens on a click and closes on the next one, and it keeps that
/// state without the caller threading it.
#[test]
fn the_chip_toggles_its_panel() {
    let id = WidgetId::from_hash("color-button-toggle");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let frame = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame(|ui| {
            ColorButton::new(color).id(id).show(ui);
        });
    };
    frame(&mut h, &mut color);
    assert_eq!(panel_nodes(&h), 0, "a chip starts closed");

    h.press_at(Vec2::new(10.0, 10.0));
    frame(&mut h, &mut color);
    h.release();
    frame(&mut h, &mut color);
    assert!(panel_nodes(&h) > 0, "the click opened the panel");

    h.advance_past_double_click(|_| {});
    h.press_at(Vec2::new(10.0, 10.0));
    frame(&mut h, &mut color);
    h.release();
    frame(&mut h, &mut color);
    assert_eq!(panel_nodes(&h), 0, "the second click closed it");
}

/// Opening the panel does not touch the colour. Only a gesture inside it
/// does.
#[test]
fn opening_the_panel_is_not_an_edit() {
    let id = WidgetId::from_hash("color-button-no-edit");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let before = color;
    let mut changed = false;
    for _ in 0..2 {
        h.frame(|ui| {
            changed |= ColorButton::new(&mut color).id(id).show(ui).changed;
        });
        h.press_at(Vec2::new(10.0, 10.0));
        h.release();
    }
    assert!(!changed);
    assert_eq!(color, before);
}
