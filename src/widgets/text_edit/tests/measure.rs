//! Measurement stability across focus transitions: a `Hug` editor's width must not snap to zero on focus with
//! an empty buffer (the placeholder is recorded with a transparent brush so the leaf has content).

use crate::widgets::text_edit::tests::*;

const SIZE: UVec2 = UVec2::new(400, 80);
const PLACEHOLDER: &str = "type something here";

fn frame(h: &mut UiHarness, buf: &mut String) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .placeholder(PLACEHOLDER)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui)
                        .response
                        .node(),
                );
            });
    };
    h.frame(&mut record);
    node.unwrap()
}

/// An empty-buffer editor in a `Hug` parent has the same width focused and unfocused.
#[test]
fn empty_editor_width_is_stable_across_focus() {
    let mut h = UiHarness::new(SIZE);
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    frame(&mut h, &mut buf);
    let node = frame(&mut h, &mut buf);
    let w_unfocused = h.ui.arranged_rect(Layer::Main, node).size.w;

    h.set_focus(id);
    frame(&mut h, &mut buf);
    let node = frame(&mut h, &mut buf);
    let w_focused = h.ui.arranged_rect(Layer::Main, node).size.w;

    assert!(
        w_unfocused > 0.0,
        "unfocused empty editor with a placeholder should have positive width, got {w_unfocused}",
    );
    assert_eq!(
        w_focused, w_unfocused,
        "focus must not change desired width; unfocused={w_unfocused} focused={w_focused}"
    );
}

const LONG: &str = "the quick brown fox jumps over the lazy dog";

/// A `container_w`-wide `Fixed` hstack holding a single-line editor sized `editor_w`, primed two frames.
fn sized_editor(h: &mut UiHarness, buf: &mut String, container_w: f32, editor_w: Sizing) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(container_w), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .size((editor_w, Sizing::fixed(40.0)))
                        .show(ui)
                        .response
                        .node(),
                );
            });
    };
    h.prime(2, &mut record);
    node.unwrap()
}

/// A `Fill`-width single-line editor shrinks below its text in a narrow container and fills a wide one: it
/// clips and scrolls, so its text uses `TextWrap::Scroll` (zero min-content) and the Fill floor is its padding.
/// A `Hug` editor still hugs its buffer, the natural-width baseline.
#[test]
fn fill_width_editor_shrinks_below_text_content() {
    const NARROW_W: f32 = 120.0;
    let mut h = UiHarness::new(UVec2::new(2100, 200));

    let mut buf = LONG.to_string();
    let hug = sized_editor(&mut h, &mut buf, 2000.0, Sizing::HUG);
    let text_w = h.ui.arranged_rect(Layer::Main, hug).size.w;
    assert!(
        text_w > NARROW_W,
        "fixture requires the text ({text_w}) to be wider than the narrow container ({NARROW_W})",
    );

    let mut buf = LONG.to_string();
    let fill = sized_editor(&mut h, &mut buf, NARROW_W, Sizing::FILL);
    let fill_w = h.ui.arranged_rect(Layer::Main, fill).size.w;
    assert_eq!(
        fill_w, NARROW_W,
        "sole Fill child must stretch to its {NARROW_W}px container, got {fill_w}"
    );
    assert!(
        fill_w < text_w,
        "Fill editor ({fill_w}) must be narrower than its text content ({text_w})",
    );
}

#[test]
fn stable_editor_uses_one_direct_layout_probe() {
    for (multiline, selected) in [(false, false), (true, true)] {
        let mut h = UiHarness::with_text(SIZE);
        let id = WidgetId::from_hash((multiline, selected));
        let mut text = String::from("editable text across two lines\nwith a selection");
        let text_len = text.len();
        let mut record = |ui: &mut Ui| {
            TextEdit::new(&mut text)
                .id(id)
                .multiline(multiline)
                .size((Sizing::fixed(240.0), Sizing::fixed(60.0)))
                .show(ui);
        };
        h.prime(2, &mut record);
        if selected {
            h.set_focus(id);
            h.ui.with_state::<TextEditState, _>(id, |_, state| {
                state.edit.selection = Some(0);
                state.edit.caret = text_len;
            });
            h.frame(&mut record);
        }
        let before = h.ui.shaper().measure_calls();
        h.frame(&mut record);
        assert_eq!(
            h.ui.shaper().measure_calls() - before,
            1,
            "multiline={multiline}, selected={selected}: measurement, caret, and selection must share one direct layout probe",
        );
    }
}

/// The placeholder takes every text form (borrowed, owned, interned, `fmt!`) and each measures the same.
#[test]
fn every_text_form_measures_the_same_placeholder() {
    use crate::primitives::text::text_input::TextInput;

    type Form = fn(&mut Ui) -> TextInput<'static>;
    let forms: [(&str, Form); 4] = [
        ("borrowed", |_| PLACEHOLDER.into()),
        ("owned", |_| PLACEHOLDER.to_owned().into()),
        ("interned", |ui| ui.intern(PLACEHOLDER).into()),
        ("fmt", |ui| {
            crate::fmt!(ui, "type {} here", "something").into()
        }),
    ];
    let mut widths = Vec::new();
    for (label, form) in forms {
        let mut h = UiHarness::new(SIZE);
        let mut buf = String::new();
        let mut node = None;
        for _ in 0..2 {
            h.frame(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        let placeholder = form(ui);
                        node = Some(
                            TextEdit::new(&mut buf)
                                .id(WidgetId::from_hash("editor"))
                                .placeholder(placeholder)
                                .size((Sizing::HUG, Sizing::HUG))
                                .show(ui)
                                .response
                                .node(),
                        );
                    });
            });
        }
        widths.push((label, h.ui.arranged_rect(Layer::Main, node.unwrap()).size.w));
    }
    let borrowed = widths[0].1;
    assert!(borrowed > 0.0, "premise: the placeholder has width");
    for (label, w) in widths {
        assert_eq!(w, borrowed, "{label}");
    }
}
