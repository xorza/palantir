use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::theme::Theme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::color_picker::swatch_row::SwatchRow;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};

fn harness() -> Harness {
    Harness::new(UVec2::new(320, 460))
}

fn frame(h: &mut Harness, id: WidgetId, color: &mut RgbaF32) -> ValueResponse {
    h.frame_value(|ui| ColorPicker::new(color).alpha(true).id(id).show(ui))
}

/// Black has no hue; dragging the value to the bottom and back must return to the starting hue.
#[test]
fn the_hue_survives_black() {
    let id = WidgetId::from_hash("picker-hue-survives-black");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);
    let start = color;

    h.press(Vec2::new(100.0, 80.0), PointerButton::Left);
    frame(&mut h, id, &mut color);
    h.drag_to(Vec2::new(1.0, 300.0));
    frame(&mut h, id, &mut color);
    assert_eq!(color.to_srgba8().r, 0, "dragged past the bottom is black");
    assert_eq!(color.to_srgba8().b, 0);

    h.drag_to(Vec2::new(190.0, 4.0));
    frame(&mut h, id, &mut color);
    h.release(PointerButton::Left);
    frame(&mut h, id, &mut color);
    let started = ColorCoords::from_color(start, ColorModel::Okhsv, 0.0).hue();
    let ended = ColorCoords::from_color(color, ColorModel::Okhsv, 0.0).hue();
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
    let edges = frame(&mut h, id, &mut color);
    assert!(
        !edges.changed,
        "the picker does not rewrite what it was handed"
    );
    assert_eq!(color.to_srgba8().r, 0xff, "and it keeps the color intact");

    h.press(Vec2::new(206.0, 1.0), PointerButton::Left);
    frame(&mut h, id, &mut color);
    let picked = color.to_srgba8();
    assert!(
        picked.r > picked.g && picked.g > picked.b,
        "an orange: {picked:?}"
    );
}

/// Opacity leaves the color channels untouched (pure blue is outside the Okhsv cube).
#[test]
fn opacity_leaves_the_color_alone() {
    let id = WidgetId::from_hash("picker-alpha-only");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x0000ff);
    frame(&mut h, id, &mut color);
    let before = color.to_srgba8();

    h.press(
        Vec2::new(120.0, 160.0 + 6.0 + 14.0 + 6.0 + 7.0),
        PointerButton::Left,
    );
    frame(&mut h, id, &mut color);
    let after = color.to_srgba8();
    assert_eq!(
        (after.r, after.g, after.b),
        (before.r, before.g, before.b),
        "pure blue must survive an opacity drag",
    );
}

#[test]
fn the_model_switch_keeps_the_color() {
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
    let mut color = RgbaF32::from_srgba8(SrgbaU8::rgb(9, 9, 9));
    let widths = |h: &mut Harness, color: &mut RgbaF32| {
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

    color = RgbaF32::from_srgba8(SrgbaU8::rgb(200, 211, 255));
    frame(&mut h, id, &mut color);
    let wide = widths(&mut h, &mut color);
    assert_eq!(wide, narrow, "the boxes followed their digits");
}

#[derive(Debug)]
struct Measured {
    field: f32,
    swatch: f32,
    /// The panel's gap, read between the field and the bars row under it.
    gap: f32,
}

/// `.style(..)` reaches the arranged field, swatches and panel gap: each equals its styled value,
/// not stock, and a caller's `.gap(..)` wins over either.
#[test]
fn the_style_reaches_the_panel_and_a_caller_gap_wins() {
    let stock = Theme::stock(ColorScheme::Dark).color_picker;
    let custom = ColorPickerTheme {
        field_height: stock.field_height + 30.0,
        swatch_size: stock.swatch_size + 7.0,
        gap: stock.gap + 5.0,
        ..stock.clone()
    };
    let measure = |style: Option<&ColorPickerTheme>, caller_gap: Option<f32>| {
        let id = WidgetId::from_hash("picker-style");
        let mut h = harness();
        let mut color = RgbaF32::hex(0x4cd3ff);
        let given = [color];
        h.frame(|ui| {
            let mut picker = ColorPicker::new(&mut color)
                .swatches(SwatchRow::Fixed(&given))
                .style(style)
                .id(id);
            if let Some(g) = caller_gap {
                picker = picker.gap(g);
            }
            picker.show(ui);
        });
        let rect = |id: WidgetId| h.rect(id).expect("arranged");
        let field = rect(id.with("field"));
        Measured {
            field: field.size.h,
            swatch: rect(id.with("swatch").with(0_usize)).size.w,
            gap: rect(id.with("bars")).min.y - field.max().y,
        }
    };

    let plain = measure(None, None);
    let styled = measure(Some(&custom), None);
    assert_eq!(plain.field, stock.field_height);
    assert_eq!(styled.field, custom.field_height);
    assert_eq!(plain.swatch, stock.swatch_size);
    assert_eq!(styled.swatch, custom.swatch_size);
    assert_eq!(plain.gap, stock.gap);
    assert_eq!(styled.gap, custom.gap);
    assert_ne!(styled.field, plain.field);
    assert_ne!(styled.swatch, plain.swatch);
    assert_ne!(styled.gap, plain.gap);

    let caller = measure(Some(&custom), Some(1.0));
    assert_eq!(caller.gap, 1.0, "a caller's gap wins over the style's");
}

/// Readouts show the bound color, not the axes' clamped reading, and a channel edit keeps the others.
#[test]
fn the_readouts_and_channel_edits_start_from_the_bound_color() {
    let id = WidgetId::from_hash("picker-bound-readout");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x0000ff);
    frame(&mut h, id, &mut color);
    frame(&mut h, id, &mut color);
    assert_eq!(
        TextEdit::draft(h.ui(), id.with("hex")),
        Some("#0000FF"),
        "the hex field shows the bound color"
    );

    let r_value = h.center_of(id.with("R").with("value"));
    h.press(id.with("R").with("value"), PointerButton::Left);
    frame(&mut h, id, &mut color);
    h.drag_to(r_value + Vec2::new(30.0, 0.0));
    frame(&mut h, id, &mut color);
    h.release(PointerButton::Left);
    frame(&mut h, id, &mut color);
    let got = color.to_srgba8();
    assert!(got.r > 0, "R moved: {got:?}");
    assert_eq!((got.g, got.b), (0, 255), "G and B kept: {got:?}");
}

/// The swatch row is hidden by default and under `SwatchRow::None`, shows the sixteen presets
/// under `History`, and the app's own colors under `Fixed`.
#[test]
fn the_swatch_row_shows_only_when_asked() {
    const OWN: [RgbaF32; 2] = [RgbaF32::WHITE, RgbaF32::BLACK];
    let id = WidgetId::from_hash("picker-history");
    for (label, row, swatches) in [
        ("default", None, None),
        ("none", Some(SwatchRow::None), None),
        ("history", Some(SwatchRow::History), Some(16)),
        ("fixed", Some(SwatchRow::Fixed(&OWN)), Some(2)),
    ] {
        let mut h = harness();
        let mut color = RgbaF32::hex(0x4cd3ff);
        h.frame(|ui| {
            let mut picker = ColorPicker::new(&mut color).id(id);
            if let Some(row) = row {
                picker = picker.swatches(row);
            }
            picker.show(ui);
        });
        let row = h
            .node_of(id.with("swatches"))
            .map(|row| h.ui().tree(row.plane.layer).children(row.node).count());
        assert_eq!(row, swatches, "{label}");
    }
}

/// Keyboard nudges commit but stay out of the history.
#[test]
fn keyboard_nudges_leave_the_history_alone() {
    use crate::widgets::color_picker::PickerState;

    let id = WidgetId::from_hash("picker-nudges");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let frame = |h: &mut Harness, color: &mut RgbaF32| {
        h.frame_value(|ui| {
            ColorPicker::new(color)
                .swatches(SwatchRow::History)
                .id(id)
                .show(ui)
                .committed
        })
    };
    frame(&mut h, &mut color);
    frame(&mut h, &mut color);
    h.click(Vec2::new(100.0, 60.0));
    frame(&mut h, &mut color);
    let row = h.state::<PickerState>(id).unwrap().history.peek().to_vec();
    assert_eq!(row[0], color, "the click is a pick");

    let mut committed = 0;
    for _ in 0..16 {
        h.key(Key::ArrowRight);
        committed += usize::from(frame(&mut h, &mut color));
    }
    assert!(committed > 0, "premise: a nudge commits");
    assert_eq!(h.state::<PickerState>(id).unwrap().history.peek(), &*row);

    // The same in a channel's value: each Up on the R chip commits a shade, none a pick.
    let red = id.with("R").with("value");
    h.ui_mut().set_focus(red);
    frame(&mut h, &mut color);
    let before = color;
    let mut committed = 0;
    for _ in 0..16 {
        h.key(Key::ArrowUp);
        committed += usize::from(frame(&mut h, &mut color));
    }
    assert_eq!(committed, 16, "premise: each nudge of the R value commits");
    assert_ne!(color, before, "premise: the nudges moved red");
    assert_eq!(h.state::<PickerState>(id).unwrap().history.peek(), &*row);

    // A value typed into the R editor and committed is a pick.
    h.key(Key::Char('9'));
    frame(&mut h, &mut color);
    h.key(Key::Enter);
    assert!(frame(&mut h, &mut color), "the typed value commits");
    assert_eq!(color.to_srgba8().r, 9);
    let history = h.state::<PickerState>(id).unwrap().history.peek().to_vec();
    assert_eq!(history[0], color, "a typed value is a pick");
    assert_eq!(history[1..], row[..history.len() - 1]);
}

/// The hex field edits a draft of its own: a blur on the shown text, or on a text spelling the
/// shown color, commits nothing; Enter on another color writes it at once; Escape drops the draft
/// and shows the color again.
#[test]
fn the_hex_field_commits_a_typed_color_and_nothing_else() {
    let id = WidgetId::from_hash("picker-hex-draft");
    let hex = id.with("hex");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    frame(&mut h, id, &mut color);
    h.ui_mut().set_focus(hex);
    frame(&mut h, id, &mut color);
    h.ui_mut().clear_focus();
    frame(&mut h, id, &mut color).assert_signals(false, false, "an unchanged blur");

    let retype = |h: &mut Harness, color: &mut RgbaF32, text: &str, end: Key| {
        h.ui_mut().set_focus(hex);
        frame(h, id, color);
        h.key(Key::End);
        for _ in 0..7 {
            h.key(Key::Backspace);
        }
        h.type_text(text);
        frame(h, id, color);
        h.key(end);
        frame(h, id, color)
    };
    retype(&mut h, &mut color, "#4cd3ff", Key::Enter).assert_signals(
        false,
        false,
        "the shown color in lower case",
    );
    assert_eq!(color, RgbaF32::hex(0x4cd3ff));

    assert!(retype(&mut h, &mut color, "#ff8800", Key::Enter).committed);
    assert_eq!(color.to_srgba8(), RgbaF32::hex(0xff8800).to_srgba8());

    retype(&mut h, &mut color, "#0000ff", Key::Escape).assert_signals(false, false, "Escape");
    assert_eq!(color.to_srgba8(), RgbaF32::hex(0xff8800).to_srgba8());
    frame(&mut h, id, &mut color);
    assert_eq!(
        TextEdit::draft(h.ui(), hex),
        Some("#FF8800"),
        "the draft dropped"
    );
}
