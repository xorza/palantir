//! The box the field keeps while editing, under scale and inside a caller's node.

use crate::Ui;
use crate::input::keyboard::key::Key;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
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
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                DragValue::new(v)
                    .editable(true)
                    .decimals(3)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .min_size((40.0, 0.0))
                    .id(id)
                    .show(ui);
            });
    };

    let mut h = UiHarness::new(surface);
    h.frame(|ui| render(ui, &mut v));
    let display_w = h.arranged(id).size.w;

    h.set_focus(id);
    h.key(Key::Enter);
    h.frame(|ui| render(ui, &mut v));
    let edit_w = h.arranged(id).size.w;

    // "1.985": five 8 px chars + 2 × 12 padding + 2 × 1 border, above the 40 px floor.
    assert_eq!(display_w, 40.0 + 24.0 + 2.0);
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
    let mut h = UiHarness::new(surface);
    let draw = |ui: &mut Ui, v: &mut f64| {
        Panel::zstack()
            .id(WidgetId::from_hash("dv-zoom-row"))
            .transform(TranslateScale::new(Vec2::ZERO, 0.5))
            .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                DragValue::new(v)
                    .editable(true)
                    .decimals(3)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .min_size((100.0, 0.0))
                    .id(id)
                    .show(ui);
            });
    };
    h.frame(|ui| draw(ui, &mut v));
    h.set_focus(id);
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
            position: bounds.position,
            max_size: bounds.max_size,
        }
    }

    /// Recorded placement of `id`, excluding padding and minimum height (each mode's own).
    #[derive(Debug, PartialEq)]
    struct Placement {
        margin: Spacing,
        align: Align,
        position: Vec2,
        max_size: Size,
    }

    const POSITION: Vec2 = Vec2::new(23.0, 11.0);
    let padding = Spacing::all(7.0);
    let margin = Spacing::all(3.0);

    let id = WidgetId::from_hash("configured-drag-value");
    let scene = |ui: &mut Ui| {
        let mut v = 1.5_f64;
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                DragValue::new(&mut v)
                    .editable(true)
                    .id(id)
                    .padding(padding)
                    .margin(margin)
                    .align(Align::CENTER)
                    .position(POSITION)
                    .min_size(Size::new(40.0, 20.0))
                    .max_size(Size::new(200.0, 60.0))
                    .show(ui);
            });
    };

    let mut h = UiHarness::new(UVec2::new(300, 100));
    h.frame(scene);
    let chip = placement(&h.ui, id);

    h.set_focus(id);
    h.key(Key::Enter);
    h.frame(scene);
    let editor = placement(&h.ui, id);

    assert_eq!(
        chip, editor,
        "edit mode dropped part of the caller's node policy \
         (margin, align, position, max_size)",
    );
    // Guards against both frames being chips and matching trivially.
    assert!(
        matches!(
            h.state::<DragValueState>(id),
            DragValueState::Editing { .. }
        ),
        "second frame must have recorded the inline editor",
    );
}

/// Clicking into the field must not change its box. An unstyled `TextEdit`
/// inherits `theme.text_edit` padding, so the editor must use the
/// `drag_value.editor` slot that `DragValueTheme::from_chip` mirrors the chip
/// padding onto. Height moved; width is pinned to the chip's last rect.
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
                    .editable(true)
                    .range(24.0..=240.0)
                    .decimals(0)
                    .suffix(" fps")
                    .size((Sizing::fixed(110.0), Sizing::HUG))
                    .id(id)
                    .show(ui);
            });
    };

    let mut h = UiHarness::new(UVec2::new(400, 120));
    h.frame(|ui| render(ui, &mut fps));
    let chip = h.arranged(id).size;

    h.set_focus(id);

    h.key(Key::Enter);
    h.frame(|ui| render(ui, &mut fps));
    let editor = h.arranged(id).size;

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
        let mut h = UiHarness::new(UVec2::new(400, 120));
        let mut fps = 120_i64;
        for _ in 0..2 {
            h.frame(|ui| {
                Panel::hstack().auto_id().show(ui, |ui| {
                    let suffix = form(ui);
                    DragValue::new(&mut fps)
                        .suffix(suffix)
                        .size((Sizing::HUG, Sizing::HUG))
                        .id(id)
                        .show(ui);
                });
            });
        }
        (label, h.arranged(id).size.w)
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
    let mut h = UiHarness::new(UVec2::new(400, 120));
    h.frame(|ui| {
        let suffix = ui.intern(" fps");
        DragValue::new(&mut fps).suffix(suffix).id(id).show(ui);
    });
    assert!(
        h.ui.state::<SuffixScratch>(id).is_some(),
        "used while the chip records"
    );
    h.frame(|_| {});
    assert!(
        h.ui.state::<SuffixScratch>(id).is_none(),
        "swept with the chip"
    );
}
