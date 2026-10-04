//! A composition in a field: shown in place, underlined, typed only on
//! commit, starting over a selection by deleting it; and the field asking
//! for IME text while it holds focus.

use crate::common::span::Span;
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::text_edit::tests::{SMALL, block_of};
use crate::widgets::theme::text_edit::TextEditTheme;

fn field() -> WidgetId {
    WidgetId::from_hash("ime-edit")
}

fn record(ui: &mut Ui, buf: &mut String) {
    TextEdit::new(buf).id(field()).show(ui);
}

/// The block's painted text, and the rects drawn wider than they are
/// tall at the caret's width — the composition's underlines, which no
/// other shape a field paints is.
fn painted(h: &UiHarness, caret_width: f32) -> (String, usize) {
    let node = h.node_of(field()).expect("recorded").node;
    let block = block_of(&h.ui, node);
    let text = h.ui.record_store().interned_text();
    let mut shown = String::new();
    let mut underlines = 0;
    for shape in h.ui.tree(Layer::Main).shapes_of(block) {
        match shape {
            ShapeRecord::Text { text: run, .. } => shown.push_str(text.resolve(run.span)),
            ShapeRecord::Quad(QuadShape::Rect {
                local_rect: Some(rect),
                ..
            }) if rect.size.h == caret_width && rect.size.w > caret_width => underlines += 1,
            _ => {}
        }
    }
    (shown, underlines)
}

/// A field focused on `text`, its caret placed `back` characters from the
/// end, settled.
fn focused_at(text: &str, back: usize) -> (UiHarness, String) {
    let mut h = UiHarness::with_text(SMALL);
    let mut buf = String::from(text);
    h.frame(|ui| record(ui, &mut buf));
    h.set_focus(field());
    h.frame(|ui| record(ui, &mut buf));
    h.key(Key::End);
    h.frame(|ui| record(ui, &mut buf));
    for _ in 0..back {
        h.key(Key::ArrowLeft);
        h.frame(|ui| record(ui, &mut buf));
    }
    (h, buf)
}

/// A composition shows spliced in at the caret, underlined, and leaves
/// the buffer alone; its commit types it there, and the underline goes.
#[test]
fn a_composition_shows_in_place_and_types_on_commit() {
    let caret_width = TextEditTheme::default().caret_width;
    let (mut h, mut buf) = focused_at("abcd", 2);
    h.on_input(InputEvent::ImePreedit(ImePreedit {
        text: "かな",
        cursor: Some(Span::new(6, 0)),
    }));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(buf, "abcd", "a composition types nothing");
    assert_eq!(painted(&h, caret_width), ("abかなcd".to_owned(), 1));

    h.on_input(InputEvent::ImeCommit("仮名"));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(buf, "ab仮名cd");
    assert_eq!(painted(&h, caret_width), ("ab仮名cd".to_owned(), 0));
}

/// A composition that starts over a selection deletes it first — once,
/// as it starts, so the next preedit of the same composition deletes
/// nothing more.
#[test]
fn a_composition_starting_over_a_selection_deletes_it() {
    let (mut h, mut buf) = focused_at("hello", 0);
    h.set_modifiers(Modifiers::CTRL);
    h.key(Key::Char('a'));
    h.frame(|ui| record(ui, &mut buf));
    h.set_modifiers(Modifiers::NONE);
    for text in ["x", "xy"] {
        h.on_input(InputEvent::ImePreedit(ImePreedit { text, cursor: None }));
        h.frame(|ui| record(ui, &mut buf));
        assert_eq!(buf, "", "{text}: the selection went, and only it");
    }
}

/// The field asks for IME text while it holds focus, with its caret in
/// screen space — inside the field's own rect, one caret wide and one
/// line tall — and stops asking once focus leaves.
#[test]
fn a_focused_field_asks_for_ime_at_its_caret() {
    let caret_width = TextEditTheme::default().caret_width;
    let (mut h, mut buf) = focused_at("abcd", 0);
    let report = h.frame(|ui| record(ui, &mut buf));
    let area = report.ime_area.expect("a focused field asks");
    let field_rect: Rect = h.ui.response_for(field()).rect.expect("arranged");
    assert!(
        field_rect.contains_rect(area),
        "{area:?} inside {field_rect:?}"
    );
    assert_eq!(area.size.w, caret_width);

    h.clear_focus();
    let report = h.frame(|ui| record(ui, &mut buf));
    assert_eq!(report.ime_area, None, "an unfocused field asks for nothing");
}
