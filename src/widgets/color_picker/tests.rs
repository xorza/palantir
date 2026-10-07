use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::okhsv::Okhsv;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::widget_core::configure::Configure;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::{UVec2, Vec2};

fn harness() -> UiHarness {
    UiHarness::with_text(UVec2::new(320, 460))
}

fn frame(h: &mut UiHarness, id: WidgetId, color: &mut RgbaF32) -> (bool, bool) {
    h.frame_value(|ui| {
        let r = ColorPicker::new(color).alpha(true).id(id).show(ui);
        (r.changed, r.committed)
    })
}

/// Black has no hue; dragging the value to the bottom and back must return to the starting hue.
#[test]
fn the_hue_survives_black() {
    let id = WidgetId::from_hash("picker-hue-survives-black");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);
    let start = color;

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
    let started = Okhsv::from_color(start, 0.0).h;
    let ended = Okhsv::from_color(color, 0.0).h;
    assert_close(
        ended,
        started,
        1e-6,
        "the hue goes through an f32 Okhsv round trip, an ulp at 0.62",
    );
}

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

    h.press_at(Vec2::new(206.0, 1.0));
    frame(&mut h, id, &mut color);
    let picked = color.to_srgba_u8();
    assert!(
        picked.r > picked.g && picked.g > picked.b,
        "an orange: {picked:?}"
    );
}

/// Opacity leaves the colour channels untouched (pure blue is outside the Okhsv cube).
#[test]
fn opacity_leaves_the_colour_alone() {
    let id = WidgetId::from_hash("picker-alpha-only");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x0000ff);
    frame(&mut h, id, &mut color);
    let before = color.to_srgba_u8();

    h.press_at(Vec2::new(120.0, 160.0 + 6.0 + 14.0 + 6.0 + 7.0));
    frame(&mut h, id, &mut color);
    let after = color.to_srgba_u8();
    assert_eq!(
        (after.r, after.g, after.b),
        (before.r, before.g, before.b),
        "pure blue must survive an opacity drag",
    );
}

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

/// The channel boxes share one width regardless of digits.
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

    color = RgbaF32::from_srgba(SrgbaU8::rgb(200, 211, 255));
    frame(&mut h, id, &mut color);
    let wide = widths(&mut h, &mut color);
    assert_eq!(wide, narrow, "the boxes followed their digits");
}

#[derive(Debug)]
struct Measured {
    field: f32,
    swatch: f32,
}

/// `.style(..)` reaches the arranged field and swatches: each equals its styled size, not stock.
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
        let rect = |id: WidgetId| h.rect(id).expect("arranged");
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

/// Readouts show the bound colour, not the axes' clamped reading, and a channel edit keeps the others.
#[test]
fn the_readouts_and_channel_edits_start_from_the_bound_colour() {
    use crate::widgets::color_picker::PickerState;

    let id = WidgetId::from_hash("picker-bound-readout");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x0000ff);
    frame(&mut h, id, &mut color);
    frame(&mut h, id, &mut color);
    assert_eq!(h.state::<PickerState>(id).hex, "#0000FF");

    let r_value = h.center_of(id.with("R").with("value"));
    h.press_on(id.with("R").with("value"));
    frame(&mut h, id, &mut color);
    h.drag_to(r_value + Vec2::new(30.0, 0.0));
    frame(&mut h, id, &mut color);
    h.release();
    frame(&mut h, id, &mut color);
    let got = color.to_srgba_u8();
    assert!(got.r > 0, "R moved: {got:?}");
    assert_eq!((got.g, got.b), (0, 255), "G and B kept: {got:?}");
}

/// The swatch row is hidden by default, shows sixteen presets under `history(true)`.
#[test]
fn history_shows_the_preset_row_only_when_asked() {
    let id = WidgetId::from_hash("picker-history");
    for (label, history, swatches) in [
        ("default", None, None),
        ("on", Some(&[true][..]), Some(16)),
        ("on then off", Some(&[true, false][..]), None),
    ] {
        let mut h = harness();
        let mut color = RgbaF32::hex(0x4cd3ff);
        h.frame(|ui| {
            let mut picker = ColorPicker::new(&mut color).id(id);
            for &on in history.unwrap_or_default() {
                picker = picker.history(on);
            }
            picker.show(ui);
        });
        let row = h
            .node_of(id.with("swatches"))
            .map(|row| h.ui.tree(row.layer).children(row.node).count());
        assert_eq!(row, swatches, "{label}");
    }
}

/// Keyboard nudges commit but stay out of the history.
#[test]
fn keyboard_nudges_leave_the_history_alone() {
    use crate::input::keyboard::key::Key;
    use crate::widgets::color_picker::PickerState;

    let id = WidgetId::from_hash("picker-nudges");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let frame = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame_value(|ui| {
            ColorPicker::new(color)
                .history(true)
                .id(id)
                .show(ui)
                .committed
        })
    };
    frame(&mut h, &mut color);
    frame(&mut h, &mut color);
    h.click_at(Vec2::new(100.0, 60.0));
    frame(&mut h, &mut color);
    let row = h.state::<PickerState>(id).history.peek().to_vec();
    assert_eq!(row[0], color, "the click is a pick");

    let mut committed = 0;
    for _ in 0..16 {
        h.key(Key::ArrowRight);
        committed += usize::from(frame(&mut h, &mut color));
    }
    assert!(committed > 0, "premise: a nudge commits");
    assert_eq!(h.state::<PickerState>(id).history.peek(), &row[..]);
}

#[test]
fn an_unchanged_hex_field_commits_nothing_on_blur() {
    let id = WidgetId::from_hash("picker-hex-blur");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);
    h.set_focus(id.with("hex"));
    frame(&mut h, id, &mut color);
    h.clear_focus();
    let (changed, committed) = frame(&mut h, id, &mut color);
    assert!(
        !changed && !committed,
        "changed {changed}, committed {committed}"
    );
}
