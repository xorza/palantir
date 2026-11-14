//! What a field measures: an empty `Hug` editor sizes to its placeholder, focused or not, a `Fill`
//! one shrinks below its text, and every placeholder text form measures alike.

use crate::primitives::layout::sizing::SizeSpec;
use crate::widgets::text_edit::tests::*;

const SIZE: UVec2 = UVec2::new(400, 80);
const PLACEHOLDER: &str = "type something here";

fn frame(h: &mut Harness, buf: &mut String) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size(SizeSpec::HUG)
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .placeholder(PLACEHOLDER)
                        .size(SizeSpec::HUG)
                        .show(ui)
                        .response
                        .node(ui),
                );
            });
    };
    h.frame(&mut record);
    node.unwrap()
}

/// An empty-buffer editor in a `Hug` parent has the same width focused and unfocused.
#[test]
fn empty_editor_width_is_stable_across_focus() {
    let mut h = Harness::mono(SIZE);
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    frame(&mut h, &mut buf);
    let node = frame(&mut h, &mut buf);
    let w_unfocused = h.ui().arranged_rect(Layer::Main, node).size.w;

    h.ui_mut().set_focus(id);
    frame(&mut h, &mut buf);
    let node = frame(&mut h, &mut buf);
    let w_focused = h.ui().arranged_rect(Layer::Main, node).size.w;

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
fn sized_editor(h: &mut Harness, buf: &mut String, container_w: f32, editor_w: Sizing) -> NodeId {
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
                        .node(ui),
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
    let mut h = Harness::mono(UVec2::new(2100, 200));

    let mut buf = LONG.to_string();
    let hug = sized_editor(&mut h, &mut buf, 2000.0, Sizing::HUG);
    let text_w = h.ui().arranged_rect(Layer::Main, hug).size.w;
    // 43 mono glyphs of 8 px, 9 px padding and 1.5 px caret room each side:
    // wider than the narrow container.
    assert_eq!(text_w, 43.0 * 8.0 + 2.0 * 9.0 + 2.0 * 1.5);

    let mut buf = LONG.to_string();
    let fill = sized_editor(&mut h, &mut buf, NARROW_W, Sizing::FILL);
    let fill_w = h.ui().arranged_rect(Layer::Main, fill).size.w;
    assert_eq!(
        fill_w, NARROW_W,
        "sole Fill child must stretch to its {NARROW_W}px container, got {fill_w}"
    );
}

#[test]
fn stable_editor_uses_one_direct_layout_probe() {
    for (multiline, selected) in [(false, false), (true, true)] {
        let mut h = Harness::new(SIZE);
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
            h.ui_mut().set_focus(id);
            h.ui_mut().with_state::<TextEditState, _>(id, |_, state| {
                state.edit.selection = Some(0);
                state.edit.caret = text_len;
            });
            h.frame(&mut record);
        }
        let before = h.ui().shaper().measure_calls();
        h.frame(&mut record);
        assert_eq!(
            h.ui().shaper().measure_calls() - before,
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
        let mut h = Harness::mono(SIZE);
        let mut buf = String::new();
        let mut node = None;
        for _ in 0..2 {
            h.frame(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size(SizeSpec::HUG)
                    .show(ui, |ui| {
                        let placeholder = form(ui);
                        node = Some(
                            TextEdit::new(&mut buf)
                                .id(WidgetId::from_hash("editor"))
                                .placeholder(placeholder)
                                .size(SizeSpec::HUG)
                                .show(ui)
                                .response
                                .node(ui),
                        );
                    });
            });
        }
        widths.push((
            label,
            h.ui().arranged_rect(Layer::Main, node.unwrap()).size.w,
        ));
    }
    let borrowed = widths[0].1;
    assert!(borrowed > 0.0, "premise: the placeholder has width");
    for (label, w) in widths {
        assert_eq!(w, borrowed, "{label}");
    }
}

/// A field over `text`, `height` its height and `max_h` its largest, and the height it takes,
/// inside the stock 5 + 5 padding.
#[derive(Debug)]
struct HeightCase {
    text: &'static str,
    height: Sizing,
    max_h: f32,
    want: f32,
}

/// The height `case`'s field takes at 14 px with a 1.4 leading: 19.6 snapped to 19.59375 on the
/// 1/64 grid, which no whole pixel holds.
fn field_height(h: &mut Harness, case: &HeightCase, multiline: bool) -> f32 {
    let mut buf = String::from(case.text);
    let mut node = None;
    h.prime(3, |ui: &mut Ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            node = Some(
                TextEdit::new(&mut buf)
                    .id(WidgetId::from_hash("editor"))
                    .font_size(14.0)
                    .line_height_factor(1.4)
                    .multiline(multiline)
                    .size((Sizing::fixed(200.0), case.height))
                    .max_size((f32::INFINITY, case.max_h))
                    .show(ui)
                    .response
                    .node(ui),
            );
        });
    });
    h.ui().arranged_rect(Layer::Main, node.unwrap()).size.h
}

/// A `Hug` field is as high as its block, under either metric: an empty one as high as its first
/// character, one line of 19.59375 px rounded up to its 20 px block, and two lines a block of
/// 39.1875 rounded up to 40, inside the stock 5 + 5 padding. An authored height wins: a `Fixed`
/// 10 and a `Hug` held under 12 keep their height and scroll the line.
#[test]
fn a_hug_field_is_as_high_as_its_block_and_an_authored_height_wins() {
    let case = |text, height, max_h, want| HeightCase {
        text,
        height,
        max_h,
        want,
    };
    let one_line = [
        case("", Sizing::HUG, f32::INFINITY, 10.0 + 20.0),
        case("x", Sizing::HUG, f32::INFINITY, 10.0 + 20.0),
        case("x", Sizing::fixed(10.0), f32::INFINITY, 10.0),
        case("x", Sizing::HUG, 12.0, 12.0),
    ];
    let two_lines = case("x\ny", Sizing::HUG, f32::INFINITY, 10.0 + 40.0);
    for metric in ["mono", "cosmic"] {
        let mut h = if metric == "mono" {
            Harness::mono(SIZE)
        } else {
            Harness::builder(SIZE).build()
        };
        for multiline in [false, true] {
            for case in &one_line {
                let got = field_height(&mut h, case, multiline);
                assert_eq!(got, case.want, "{metric}, multiline {multiline}: {case:?}");
            }
        }
        assert_eq!(
            field_height(&mut h, &two_lines, true),
            two_lines.want,
            "{metric}"
        );
    }
}
