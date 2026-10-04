//! IME text: commits typed in place among the key presses, split between
//! characters, and the preedit a focused widget reads.

use crate::common::span::Span;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text_edit::TextEdit;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(300, 80);

fn field() -> WidgetId {
    WidgetId::from_hash("ime-field")
}

/// A focused, empty field, settled.
fn focused_field() -> (UiHarness, String) {
    let mut h = UiHarness::with_text(SURFACE);
    let mut buf = String::new();
    h.frame(|ui| record(ui, &mut buf));
    h.set_focus(field());
    h.frame(|ui| record(ui, &mut buf));
    (h, buf)
}

fn record(ui: &mut Ui, buf: &mut String) {
    TextEdit::new(buf).id(field()).show(ui);
}

/// While IME is on, plain typing arrives as commits, so a commit, a
/// Backspace and a commit in one frame must apply in that order: `a`,
/// erased, then `b`. Applied before the keys, the commits would leave
/// `a` erased by nothing and `b` erased by Backspace.
#[test]
fn commits_and_keys_type_in_arrival_order() {
    let (mut h, mut buf) = focused_field();
    h.on_input(InputEvent::ImeCommit("a"));
    h.key(Key::Backspace);
    h.on_input(InputEvent::ImeCommit("b"));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(buf, "b");
}

/// A commit longer than one press holds splits between characters — 3
/// bytes each here, so four to a 14-byte press — and arrives whole, with
/// control characters dropped and held modifiers ignored: Ctrl held while
/// an input method commits `z` types `z` rather than undoing.
#[test]
fn a_commit_splits_between_characters_and_types_whole() {
    let (mut h, mut buf) = focused_field();
    let text = "日本語の文章を入力する";
    h.set_modifiers(Modifiers::CTRL);
    h.on_input(InputEvent::ImeCommit("日本語の文章を入力する\tz\n"));
    let pieces = h.frame_value(|ui| {
        let pieces: Vec<String> = ui
            .keyboard_events()
            .iter()
            .map(|press| press.text.as_str().to_owned())
            .collect();
        record(ui, &mut buf);
        pieces
    });
    // 11 three-byte characters and `z`: 4 + 4 + 3 characters, the third
    // piece taking `z` too (9 + 1 bytes).
    assert_eq!(pieces, ["日本語の", "文章を入", "力するz"]);
    assert_eq!(buf, format!("{text}z"));
}

/// The preedit is the focused widget's: read back as the last event left
/// it, with its cursor as a byte range, gone once it commits, and gone
/// for a widget that no longer holds focus.
#[test]
fn the_preedit_is_a_level_for_the_focused_widget() {
    let (mut h, mut buf) = focused_field();
    h.on_input(InputEvent::ImePreedit {
        text: "かな",
        cursor: Some(Span::new(3, 3)),
    });
    h.frame(|ui| record(ui, &mut buf));
    let preedit = h.ui.ime_preedit().expect("a live composition");
    assert_eq!((preedit.text, preedit.cursor), ("かな", Some(3..6)));
    assert_eq!(buf, "", "a preedit types nothing");

    h.on_input(InputEvent::ImeCommit("仮名"));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(h.ui.ime_preedit(), None, "a commit ends it");
    assert_eq!(buf, "仮名");

    h.on_input(InputEvent::ImePreedit {
        text: "か",
        cursor: None,
    });
    h.frame(|ui| record(ui, &mut buf));
    assert!(h.ui.ime_preedit().is_some());
    h.clear_focus();
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(h.ui.ime_preedit(), None, "it is the focused widget's alone");
}

/// A commit that arrives behind a held key waits with it, and keeps its
/// text across the wait — the host's string is gone by then, so the queue
/// copied it — and still types after the key it waited behind.
#[test]
fn a_held_commit_keeps_its_text() {
    let (mut h, mut buf) = focused_field();
    h.on_input(InputEvent::ImeCommit("ab"));
    // Two command keys: the second waits a frame, and so does everything
    // after it.
    h.key(Key::ArrowLeft);
    h.key(Key::ArrowLeft);
    let owned = String::from("xy");
    h.on_input(InputEvent::ImeCommit(&owned));
    drop(owned);
    h.frame(|ui| record(ui, &mut buf));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(
        buf, "xyab",
        "typed where the second ArrowLeft left the caret"
    );
}
