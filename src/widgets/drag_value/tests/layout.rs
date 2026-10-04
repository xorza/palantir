//! The box the field keeps while editing, under scale and inside a caller's
//! node.

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

    // A `Hug` row makes the field's own content drive its width — the
    // condition where the width-cap matters. The chip shows "1.985"; the
    // editor seeds the full-precision value on entry and must scroll it
    // inside the chip's width rather than grow the row.
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

    // Enter edit mode; entry seeds the full-precision text.
    h.set_focus(id);
    h.key(Key::Enter);
    h.frame(|ui| render(ui, &mut v));
    let edit_w = h.arranged(id).size.w;

    // "1.985": five 8 px mono chars, 2 × 12 padding, 2 × 1 border — above
    // the 40 px floor.
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

    // A scaled parent (0.5×) halves the chip's post-transform rect to ~60px
    // while `min_size` is 100 — the cap must read the pre-transform
    // (logical, 120) width and floor at `min_size`, else feeding the 60px
    // post-transform width makes `AxisSlot::resolve`'s `clamp(100, 60)`
    // panic.
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

/// Entering edit mode must not move, resize, or re-place the widget.
///
/// The chip and the inline editor are two different widgets sharing one
/// `WidgetId`, so every field of the caller's `Node` that positions the
/// widget in its parent has to be carried across the swap by hand. It
/// wasn't: padding, margin, alignment, canvas position and grid placement
/// were all dropped, so clicking to type visibly jumped the field.
///
/// Rather than enumerate the fields a second time, this records the same
/// configured `DragValue` twice — once as a chip, once focused as an
/// editor — and asserts the *recorded* layout matches. Any inherited field
/// that stops being carried shows up here as a divergence, including
/// fields added later.
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

    /// Recorded placement of `id` — the fields the swap must preserve.
    ///
    /// Padding and minimum height are excluded on purpose: both modes
    /// resolve those from their own theme and intrinsics (a text editor has
    /// a line-height floor a chip does not), so they are not carried by the
    /// node policy and comparing them would pin theme configuration rather
    /// than this fix.
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
    // A `Canvas` parent so `position` is honoured rather than ignored.
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

    // Focus flips the same widget to its inline editor.
    h.set_focus(id);
    h.key(Key::Enter);
    h.frame(scene);
    let editor = placement(&h.ui, id);

    assert_eq!(
        chip, editor,
        "edit mode dropped part of the caller's node policy \
         (margin, align, position, max_size)",
    );
    // Guard against the test going inert if the editor path stops being
    // taken at all — then both frames would be chips and match trivially.
    assert!(
        matches!(
            h.state::<DragValueState>(id),
            DragValueState::Editing { .. }
        ),
        "second frame must have recorded the inline editor",
    );
}

/// Clicking into the field must not change its box.
///
/// The chip and the inline editor are two different widgets, and an
/// unstyled
/// `TextEdit` inherits `theme.text_edit` — a standalone text field's box,
/// whose
/// padding is not the chip's. `DragValueTheme::from_chip` mirrors the
/// chip's
/// padding onto `drag_value.editor` for exactly this reason, but nothing
/// pointed
/// the editor at that slot, so the whole mirror was dead and the field
/// resized
/// on click (the showcase's 120 fps row lost 5 px of height).
///
/// Height is the axis that moves: the width is already pinned to the chip's
/// last rect, so only the vertical padding difference showed.
#[test]
fn entering_edit_mode_keeps_the_chips_box() {
    let id = WidgetId::from_hash("dv-box");
    let mut fps = 120_i64;
    // A `Hug` height is what exposes the difference — a fixed one would pin
    // both modes to the same number whatever their padding resolved to.
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

/// The suffix takes every form of text a widget takes, and each one
/// labels the chip the same: borrowed, owned, interned this pass, and
/// `fmt!` output. The interned form is copied out of the arena while the
/// label is formatted into it, which is the case a plain `&str` never
/// meets. A chip with no suffix is narrower, so the width reads the
/// suffix.
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

/// The copy an interned suffix is formatted through lives on the chip's
/// own id, so it leaves with the chip rather than outliving it.
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
