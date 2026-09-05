use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::color::okhsv::Okhsv;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::configure::Configure;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::{UVec2, Vec2};

/// Wide enough for the panel and its rows to lay out without wrapping.
fn harness() -> UiHarness {
    UiHarness::with_text(UVec2::new(320, 460))
}

fn frame(h: &mut UiHarness, id: WidgetId, color: &mut RgbaF32) -> (bool, bool) {
    h.frame_value(|ui| {
        let r = ColorPicker::new(color).alpha(true).id(id).show(ui);
        (r.changed, r.committed)
    })
}

/// Black has no hue, so a picker that re-read its axes from the colour every
/// frame would lose it. Dragging the value to the bottom and back has to come
/// back to the hue it started on.
#[test]
fn the_hue_survives_black() {
    let id = WidgetId::from_hash("picker-hue-survives-black");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);
    let start = color;

    // Down past the bottom of the field to black, then back up.
    h.press_at(Vec2::new(100.0, 80.0));
    frame(&mut h, id, &mut color);
    h.drag_to(Vec2::new(1.0, 300.0));
    frame(&mut h, id, &mut color);
    assert_eq!(color.to_srgba_u8().r, 0, "dragged past the bottom is black");
    assert_eq!(color.to_srgba_u8().b, 0);

    h.drag_to(Vec2::new(190.0, 4.0));
    frame(&mut h, id, &mut color);
    h.release();
    frame(&mut h, id, &mut color);
    // The shade differs — the pointer went somewhere else — but the hue is
    // the one the picker held before it passed through black.
    let started = Okhsv::from_color(start, 0.0).h;
    let ended = Okhsv::from_color(color, 0.0).h;
    assert!(
        (started - ended).abs() < 1e-3,
        "hue {started} came back as {ended} ({:?})",
        color.to_srgba_u8(),
    );
}

/// A colour written into the binding from outside moves the handles, which is
/// how a caller's own edit reaches the picker.
#[test]
fn an_outside_edit_re_seeds_the_axes() {
    let id = WidgetId::from_hash("picker-outside-edit");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);

    color = RgbaF32::hex(0xff8800);
    let (changed, _) = frame(&mut h, id, &mut color);
    assert!(!changed, "the picker does not rewrite what it was handed");
    assert_eq!(
        color.to_srgba_u8().r,
        0xff,
        "and it keeps the colour intact"
    );

    // The axes now describe the new colour: pressing the field's top-right
    // corner lands on that hue's most saturated colour, not the old one's.
    h.press_at(Vec2::new(206.0, 1.0));
    frame(&mut h, id, &mut color);
    let picked = color.to_srgba_u8();
    assert!(
        picked.r > picked.g && picked.g > picked.b,
        "an orange: {picked:?}"
    );
}

/// Moving the opacity leaves the three colour channels exactly where they
/// were. It has to: a wedge of sRGB around pure blue is outside the Okhsv
/// cube, so rebuilding the colour from the axes would shift it.
#[test]
fn opacity_leaves_the_colour_alone() {
    let id = WidgetId::from_hash("picker-alpha-only");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x0000ff);
    frame(&mut h, id, &mut color);
    let before = color.to_srgba_u8();

    // The alpha bar sits under the hue bar, right of the preview chip.
    h.press_at(Vec2::new(120.0, 160.0 + 6.0 + 14.0 + 6.0 + 7.0));
    frame(&mut h, id, &mut color);
    let after = color.to_srgba_u8();
    assert_eq!(
        (after.r, after.g, after.b),
        (before.r, before.g, before.b),
        "pure blue must survive an opacity drag",
    );
}

/// The model is retained across frames, and switching it keeps the colour.
#[test]
fn the_model_switch_keeps_the_colour() {
    let id = WidgetId::from_hash("picker-model-switch");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    h.frame(|ui| {
        ColorPicker::new(&mut color)
            .model(ColorModel::Hsv)
            .id(id)
            .show(ui);
    });
    let pinned = color;
    h.frame(|ui| {
        ColorPicker::new(&mut color)
            .model(ColorModel::Okhsv)
            .id(id)
            .show(ui);
    });
    assert_eq!(color, pinned, "a pinned model change is not an edit");
}

/// The four channel boxes are one width, and that width does not follow the
/// digits inside them. A number going from 99 to 100 under a drag would
/// otherwise shuffle every box beside it.
#[test]
fn the_channel_boxes_are_one_fixed_width() {
    let id = WidgetId::from_hash("picker-fixed-values");
    let mut h = harness();
    let mut color = RgbaF32::from_srgba(SrgbaU8::rgb(9, 9, 9));
    let widths = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame_value(|ui| {
            ColorPicker::new(color).alpha(true).id(id).show(ui);
            ["R", "G", "B", "S"].map(|name| {
                ui.response_for(id.with(name).with("value"))
                    .layout_rect
                    .expect("the value box laid out")
                    .size
                    .w
            })
        })
    };
    frame(&mut h, id, &mut color);
    frame(&mut h, id, &mut color);
    let narrow = widths(&mut h, &mut color);
    assert!(narrow[0] > 0.0, "the boxes have a width at all");
    for (name, width) in ["R", "G", "B", "S"].iter().zip(narrow) {
        assert_eq!(width, narrow[0], "{name} is a different width");
    }

    // Three digits everywhere instead of one: the same width, still.
    color = RgbaF32::from_srgba(SrgbaU8::rgb(200, 211, 255));
    frame(&mut h, id, &mut color);
    let wide = widths(&mut h, &mut color);
    assert_eq!(wide, narrow, "the boxes followed their digits");
}

/// What one picker measured: the field's height and a swatch's side.
#[derive(Debug)]
struct Measured {
    field: f32,
    swatch: f32,
}

/// `.style(..)` on the picker reaches the widgets it arranges, not only the
/// values and labels it records itself. The field and the swatch row took
/// their sizes from the global theme before, so a styled picker showed a
/// stock field over styled numbers.
///
/// Differential, so no value is baked in: the field is exactly the styled
/// height and a swatch exactly the styled side, and each differs from stock.
#[test]
fn the_style_reaches_the_field_and_the_swatches() {
    let stock = ColorPickerTheme::default();
    let custom = ColorPickerTheme {
        field_height: stock.field_height + 30.0,
        swatch_size: stock.swatch_size + 7.0,
        ..stock.clone()
    };
    let measure = |style: Option<&ColorPickerTheme>| {
        let id = WidgetId::from_hash("picker-style");
        let mut h = harness();
        let mut color = RgbaF32::hex(0x4cd3ff);
        let given = [color];
        h.frame(|ui| {
            ColorPicker::new(&mut color)
                .swatches(&given)
                .style(style)
                .id(id)
                .show(ui);
        });
        let rect = |id: WidgetId| h.ui.response_for(id).rect.expect("arranged");
        Measured {
            field: rect(id.with("field")).size.h,
            swatch: rect(id.with("swatch").with(0_usize)).size.w,
        }
    };

    let plain = measure(None);
    let styled = measure(Some(&custom));
    assert_eq!(plain.field, stock.field_height);
    assert_eq!(styled.field, custom.field_height);
    assert_eq!(plain.swatch, stock.swatch_size);
    assert_eq!(styled.swatch, custom.swatch_size);
    assert_ne!(styled.field, plain.field);
    assert_ne!(styled.swatch, plain.swatch);
}
