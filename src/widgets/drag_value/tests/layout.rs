//! The box the field keeps while editing, under scale and inside a caller's node.

use crate::host::harness::Harness;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::scene::layer::Layer;
use crate::shape::record::ShapeRecord;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::drag_value::{DragValue, DragValueState};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn editing_a_long_value_holds_the_field_width() {
    let surface = UVec2::new(400, 120);
    let id = WidgetId::from_hash("dv-width");
    let mut v = 1.984_573_845_634_985_2_f64;

    // A `Hug` row lets the chip drive the width: the editor seeds the
    // full-precision value and must scroll within the chip's width.
    let render = |ui: &mut Ui, v: &mut f64| {
        Panel::hstack()
            .id(WidgetId::from_hash("dv-row"))
            .size(SizeSpec::HUG)
            .show(ui, |ui| {
                DragValue::new(v)
                    .decimals(3)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .min_size((40.0, 0.0))
                    .id(id)
                    .show(ui);
            });
    };

    let mut h = Harness::mono(surface);
    h.frame(|ui| render(ui, &mut v));
    let display_w = h.layout_rect(id).unwrap().size.w;

    h.ui_mut().set_focus(id);
    h.key(Key::Enter);
    h.frame(|ui| render(ui, &mut v));
    let edit_w = h.layout_rect(id).unwrap().size.w;

    // "1.985": five 8 px chars + 2 × 1.5 caret reserve + 2 × 12 padding + 2 × 1 border, above
    // the 40 px floor.
    assert_eq!(display_w, 40.0 + 3.0 + 24.0 + 2.0);
    assert_eq!(
        display_w, edit_w,
        "editing the full-precision value must not resize the field \
         (display {display_w}, edit {edit_w})"
    );
}

#[test]
fn editing_under_a_scaled_canvas_does_not_panic() {
    let surface = UVec2::new(400, 120);
    let id = WidgetId::from_hash("dv-zoom");
    let mut v = 1.984_573_845_634_985_2_f64;

    // A 0.5× parent halves the chip's rect to ~60 while `min_size` is 100: the
    // cap must read the logical width or `AxisSlot::resolve`'s `clamp(100, 60)` panics.
    let mut h = Harness::mono(surface);
    let draw = |ui: &mut Ui, v: &mut f64| {
        Panel::zstack()
            .id(WidgetId::from_hash("dv-zoom-row"))
            .transform(TranslateScale::new(Vec2::ZERO, 0.5))
            .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                DragValue::new(v)
                    .decimals(3)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .min_size((100.0, 0.0))
                    .id(id)
                    .show(ui);
            });
    };
    h.frame(|ui| draw(ui, &mut v));
    h.ui_mut().set_focus(id);
    h.key(Key::Enter);
    h.frame(|ui| draw(ui, &mut v));
}

/// Entering edit mode must not move, resize or re-place the widget. Chip and
/// editor share one `WidgetId`, so every positioning field of the caller's
/// `Node` must carry across. Records the same `DragValue` as chip and editor
/// and compares the layout, so later fields are covered.
#[test]
fn entering_edit_mode_preserves_the_callers_node_placement() {
    fn placement(ui: &Ui, id: WidgetId) -> Placement {
        let node = ui.cascade().endpoint(id).expect("drag value node").node;
        let tree = ui.tree(Layer::Main);
        let layout = tree.records.layout()[node.idx()];
        let bounds = tree.bounds(node);
        Placement {
            margin: layout.margin,
            align: layout.meta.align(),
            pos: bounds.pos,
            max_size: bounds.max_size,
        }
    }

    /// Recorded placement of `id`, excluding padding and minimum height (each mode's own).
    #[derive(Debug, PartialEq)]
    struct Placement {
        margin: Spacing,
        align: Align,
        pos: Vec2,
        max_size: Size,
    }

    const POS: Vec2 = Vec2::new(23.0, 11.0);
    let padding = Spacing::all(7.0);
    let margin = Spacing::all(3.0);

    let id = WidgetId::from_hash("configured-drag-value");
    let scene = |ui: &mut Ui| {
        let mut v = 1.5_f64;
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                DragValue::new(&mut v)
                    .id(id)
                    .padding(padding)
                    .margin(margin)
                    .align(Align::CENTER)
                    .at(POS)
                    .min_size(Size::new(40.0, 20.0))
                    .max_size(Size::new(200.0, 60.0))
                    .show(ui);
            });
    };

    let mut h = Harness::mono(UVec2::new(300, 100));
    h.frame(scene);
    let chip = placement(h.ui(), id);

    h.ui_mut().set_focus(id);
    h.key(Key::Enter);
    h.frame(scene);
    let editor = placement(h.ui(), id);

    assert_eq!(
        chip, editor,
        "edit mode dropped part of the caller's node policy \
         (margin, align, position, max_size)",
    );
    // Guards against both frames being chips and matching trivially.
    assert!(
        matches!(
            h.state::<DragValueState>(id).unwrap(),
            DragValueState::Editing { .. }
        ),
        "second frame must have recorded the inline editor",
    );
}

/// Clicking into the field must not change its box. An unstyled `TextEdit`
/// inherits `theme.text_edit` padding, so the editor wears
/// `DragValueTheme::editor`, the chip's own looks and padding. Height moved; width is pinned to
/// the chip's last rect.
#[test]
fn entering_edit_mode_keeps_the_chips_box() {
    let id = WidgetId::from_hash("dv-box");
    let mut fps = 120_i64;
    let render = |ui: &mut Ui, v: &mut i64| {
        Panel::hstack()
            .id(WidgetId::from_hash("dv-box-row"))
            .gap(8.0)
            .show(ui, |ui| {
                DragValue::new(v)
                    .range(24.0..=240.0)
                    .decimals(0)
                    .suffix(" fps")
                    .size((Sizing::fixed(110.0), Sizing::HUG))
                    .id(id)
                    .show(ui);
            });
    };

    let mut h = Harness::mono(UVec2::new(400, 120));
    h.frame(|ui| render(ui, &mut fps));
    let chip = h.layout_rect(id).unwrap().size;

    h.ui_mut().set_focus(id);

    h.key(Key::Enter);
    h.frame(|ui| render(ui, &mut fps));
    let editor = h.layout_rect(id).unwrap().size;

    assert_eq!(
        (chip.w, chip.h),
        (editor.w, editor.h),
        "entering edit mode resized the field (chip {chip:?}, editor {editor:?})",
    );
}

/// The suffix takes every form of text (borrowed, owned, interned, `fmt!`) and
/// each labels the chip the same; the interned form is copied out of the arena
/// while formatted into it.
#[test]
fn every_text_form_labels_the_same_suffix() {
    use crate::primitives::text::text_input::TextInput;

    type Form = fn(&mut Ui) -> TextInput<'static>;
    let forms: [(&str, Form); 5] = [
        ("none", |_| TextInput::default()),
        ("borrowed", |_| " fps".into()),
        ("owned", |_| " fps".to_owned().into()),
        ("interned", |ui| ui.intern(" fps").into()),
        ("fmt", |ui| crate::fmt!(ui, " {}", "fps").into()),
    ];
    let id = WidgetId::from_hash("dv-suffix");
    let widths = forms.map(|(label, form)| {
        let mut h = Harness::mono(UVec2::new(400, 120));
        let mut fps = 120_i64;
        for _ in 0..2 {
            h.frame(|ui| {
                Panel::hstack().auto_id().show(ui, |ui| {
                    let suffix = form(ui);
                    DragValue::new(&mut fps)
                        .suffix(suffix)
                        .size(SizeSpec::HUG)
                        .id(id)
                        .show(ui);
                });
            });
        }
        (label, h.layout_rect(id).unwrap().size.w)
    });
    let [none, borrowed, rest @ ..] = widths;
    assert!(borrowed.1 > none.1, "premise: the suffix widens the chip");
    for (label, w) in rest {
        assert_eq!(w, borrowed.1, "{label}");
    }
}

#[test]
fn the_suffix_copy_leaves_with_the_chip() {
    use crate::widgets::drag_value::SuffixScratch;

    let id = WidgetId::from_hash("dv-leaving-suffix");
    let mut fps = 120_i64;
    let mut h = Harness::mono(UVec2::new(400, 120));
    h.frame(|ui| {
        let suffix = ui.intern(" fps");
        DragValue::new(&mut fps).suffix(suffix).id(id).show(ui);
    });
    assert!(
        h.ui().state::<SuffixScratch>(id).is_some(),
        "used while the chip records"
    );
    h.frame(|_| {});
    assert!(
        h.ui().state::<SuffixScratch>(id).is_none(),
        "swept with the chip"
    );
}

/// The chip records its value at the alignment `text_align` sets, centered by default.
#[test]
fn text_align_places_the_value_in_the_chip() {
    let id = WidgetId::from_hash("dv-align");
    let recorded = |align: Option<Align>| {
        let mut h = Harness::mono(UVec2::new(200, 60));
        let mut v = 0.5_f64;
        h.frame(|ui| {
            let chip = DragValue::new(&mut v)
                .id(id)
                .size((Sizing::fixed(100.0), Sizing::HUG));
            match align {
                Some(align) => chip.text_align(align),
                None => chip,
            }
            .show(ui);
        });
        let node = h.node_for_widget_id(id);
        h.ui()
            .tree(Layer::Main)
            .shapes_of(node)
            .find_map(|shape| match shape {
                ShapeRecord::Text { align, .. } => Some(*align),
                _ => None,
            })
            .expect("the chip records its value")
    };
    assert_eq!(recorded(None), Align::CENTER);
    assert_eq!(recorded(Some(Align::LEFT)), Align::LEFT);
}

/// The value's glyphs keep their place when the editor takes the chip's box, under every
/// alignment and with an authored padding: the chip keeps the caret reserve the editor keeps
/// around its glyphs, and the editor wears the chip's padding.
#[test]
fn entering_edit_mode_keeps_the_glyphs_in_place() {
    use crate::internals::paint_capture::PaintCall;
    use crate::primitives::geometry::rect::Rect;

    fn glyphs(h: &Harness) -> Rect {
        h.encode_paint()
            .calls
            .iter()
            .find_map(|call| match call {
                PaintCall::Text(text) => Some(text.rect),
                _ => None,
            })
            .expect("the value paints")
    }

    let id = WidgetId::from_hash("dv-glyphs");
    // Stock theme: padding (12, 4) inside a 1 px ring, so a 100 px chip holds its value in
    // 100 − 2 × 13 = 74 px from x = 13; an authored 6 px padding, 100 − 2 × 7 = 86 px from x = 7.
    // A 1.5 px caret reserves 3 px: after left- and right-aligned values, half on each side of
    // a centered one, which so keeps the box's center.
    let paddings = [(None, 13.0, 74.0), (Some(Spacing::all(6.0)), 7.0, 86.0)];
    for (padding, inset, inner) in paddings {
        for align in [Align::LEFT, Align::CENTER, Align::RIGHT] {
            let mut v = 189_i64;
            let render = |ui: &mut Ui, v: &mut i64| {
                Panel::hstack().auto_id().show(ui, |ui| {
                    let chip = DragValue::new(v)
                        .text_align(align)
                        .size((Sizing::fixed(100.0), Sizing::HUG))
                        .id(id);
                    match padding {
                        Some(padding) => chip.padding(padding),
                        None => chip,
                    }
                    .show(ui);
                });
            };
            let mut h = Harness::new(UVec2::new(200, 60));
            h.prime(2, |ui| render(ui, &mut v));
            let chip = glyphs(&h);
            h.ui_mut().set_focus(id);
            h.key(Key::Enter);
            h.prime(2, |ui| render(ui, &mut v));
            assert!(
                matches!(
                    h.state::<DragValueState>(id),
                    Some(DragValueState::Editing { .. })
                ),
                "{padding:?} {align:?}: the editor shows",
            );
            let editor = glyphs(&h);

            let label = format!("{padding:?} {align:?}");
            let w = chip.size.w;
            let expected = if align == Align::LEFT {
                inset
            } else if align == Align::CENTER {
                inset + (inner - w) / 2.0
            } else {
                inset + inner - 3.0 - w
            };
            assert_eq!(chip.min.x, expected, "{label}: the chip's value");
            assert_eq!(chip.min, editor.min, "{label}: the glyphs moved on edit");
            assert_eq!(chip.size, editor.size, "{label}: the run changed on edit");
        }
    }
}
