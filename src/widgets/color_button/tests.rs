use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::color_button::ColorButton;
use crate::widgets::configure::Configure;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::{UVec2, Vec2};

fn harness() -> UiHarness {
    UiHarness::with_text(UVec2::new(420, 560))
}

/// How many nodes the popup layer holds — zero while the chip is closed.
fn panel_nodes(h: &UiHarness) -> usize {
    h.ui.layout(Layer::Popup).rect.len()
}

/// One click on a chip at the surface's corner, with a frame between the
/// press and the release so the chip sees both edges.
fn click_chip(h: &mut UiHarness, mut frame: impl FnMut(&mut UiHarness)) {
    h.press_at(Vec2::new(10.0, 10.0));
    frame(h);
    h.release();
    frame(h);
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

    click_chip(&mut h, |h| frame(h, &mut color));
    assert!(panel_nodes(&h) > 0, "the click opened the panel");

    h.advance_past_double_click(|_| {});
    click_chip(&mut h, |h| frame(h, &mut color));
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

/// What one open popup measured: its chrome's corners, its body height, and
/// the side of the preview chip inside it.
#[derive(Debug)]
struct Opened {
    corners: Corners,
    height: f32,
    preview: f32,
}

/// Open the chip under `style` and measure what it dropped.
fn open_with(style: Option<&ColorPickerTheme>) -> Opened {
    let id = WidgetId::from_hash("color-button-theme");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let mut frame = |h: &mut UiHarness| {
        h.frame(|ui| {
            ColorButton::new(&mut color).style(style).id(id).show(ui);
        });
    };
    frame(&mut h);
    click_chip(&mut h, &mut frame);

    let body_id = id.with("panel");
    let tree = h.ui.tree(Layer::Popup);
    let body = tree
        .records
        .widget_id()
        .iter()
        .position(|w| *w == body_id)
        .expect("popup body recorded");
    let chrome = tree
        .chrome(NodeId(body as u32))
        .expect("the popup body paints chrome");
    let rect = |id: WidgetId| h.ui.response_for(id).rect.expect("arranged");
    Opened {
        corners: chrome.corners,
        height: rect(body_id).size.h,
        preview: rect(id.with("picker").with("preview")).size.h,
    }
}

/// The popup wears the picker theme's chrome and gutter, and the panel inside
/// it takes the same bundle — one `.style(..)` on the chip restyles the whole
/// of what it drops. The popup painted nothing before, and the panel inside
/// ignored the chip's style.
///
/// Differential, so no value is baked in: against the stock theme, the styled
/// body is taller by exactly the padding it gained on two edges plus what the
/// preview chip grew by, and its corners are the styled radius.
#[test]
fn the_popup_takes_the_picker_theme() {
    let stock = ColorPickerTheme::default();
    let custom = ColorPickerTheme {
        // Same stroke as stock, so the band the tree folds into the padding
        // is the same on both sides of the difference.
        popup: Background {
            corners: Corners::all(9.0),
            ..stock.popup.clone()
        },
        popup_padding: Spacing::all(19.0),
        chip_size: stock.chip_size + 30.0,
        ..stock.clone()
    };
    assert_ne!(custom.popup.corners, stock.popup.corners);

    let plain = open_with(None);
    let styled = open_with(Some(&custom));

    assert_eq!(plain.corners, stock.popup.corners, "stock chrome");
    assert_eq!(styled.corners, custom.popup.corners, "styled chrome");
    assert_eq!(plain.preview, stock.chip_size, "stock preview");
    assert_eq!(styled.preview, custom.chip_size, "styled preview");

    // Two edges of padding, (19 - 8) * 2 = 22, plus the preview's 30: the chip
    // is taller than the bar beside it on both sides of the difference, so
    // the bars row grows by exactly what the chip does.
    let padding = custom.popup_padding.vert() - stock.popup_padding.vert();
    assert_eq!(styled.height - plain.height, padding + 30.0);
}
