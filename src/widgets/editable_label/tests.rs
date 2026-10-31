use glam::{UVec2, Vec2};

use crate::display::Display;
use crate::host::harness::Harness;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::text_edit::TextEditTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::editable_label::EditableLabel;

/// What one frame of a label reported, from its last pass.
#[derive(Debug, Default, PartialEq)]
struct Shown {
    editing: bool,
    clicked: bool,
    drag_started: bool,
    committed_text: Option<String>,
}

fn record(ui: &mut Ui, id: WidgetId, editable: bool) -> Shown {
    let response = EditableLabel::new("Alpha")
        .id(id)
        .editable(editable)
        .show(ui);
    Shown {
        editing: response.editing,
        clicked: response.response.clicked(),
        drag_started: response.response.left.drag.started(),
        committed_text: response.committed_text,
    }
}

/// A harness whose display does not snap, so rects compare exactly.
fn harness() -> Harness {
    let surface = UVec2::new(300, 100);
    let mut h = Harness::builder(surface).mono().build();
    h.set_display(Display {
        physical: surface,
        pixel_snap: false,
        ..Display::default()
    });
    h
}

/// Double-click the label `id` and run the frame its editor opens in.
fn open(h: &mut Harness, id: WidgetId, editable: bool) {
    let hit = h.rect(id).expect("label arranged").center();
    for _ in 0..2 {
        h.click(hit);
        h.frame(|ui| {
            record(ui, id, editable);
        });
    }
    h.frame(|ui| {
        record(ui, id, editable);
    });
}

/// The label holds its rect across the swap to its editor and back, for the default theme, with
/// its padding and border, for a flat one, and for a padding the caller sets; in the flat one the
/// text sits on the label's leading edge, where the editor draws it.
#[test]
fn the_label_and_its_editor_take_one_rect() {
    let flat = {
        let mut theme = TextEditTheme::default();
        theme.defaults = SlotDefaults {
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            ..theme.defaults
        };
        for look in theme.looks.each_mut() {
            look.chrome = Chrome::NONE;
        }
        theme
    };
    for (name, style, padding) in [
        ("default", None, None),
        ("flat", Some(&flat), None),
        ("padded", None, Some(Spacing::new(9.0, 3.0, 5.0, 7.0))),
    ] {
        let id = WidgetId::from_hash(("label-rect", name));
        let mut h = harness();
        let show = |ui: &mut Ui| {
            let mut label = EditableLabel::new("Ab").id(id).style(style);
            if let Some(padding) = padding {
                label = label.padding(padding);
            }
            label.show(ui).editing
        };
        assert!(!h.frame_value(show));
        let idle = h.rect(id).expect("label arranged");
        if style.is_some() {
            let text = h.rect(EditableLabel::label_id(id)).expect("text arranged");
            assert_eq!(text.min.x, idle.min.x, "{name}: text on the leading edge");
        }
        let hit = idle.center();
        for _ in 0..2 {
            h.click(hit);
            h.frame_value(show);
        }
        assert!(h.frame_value(show), "{name}: the editor opened");
        assert_eq!(h.rect(id), Some(idle), "{name}: the editor's rect");
        // The first Escape drops the selection the editor opened with, the second the edit.
        for _ in 0..2 {
            h.key(Key::Escape);
            h.frame_value(show);
        }
        assert!(!h.frame_value(show), "{name}: Escape closed the editor");
        assert_eq!(h.rect(id), Some(idle), "{name}: the label's rect again");
    }
}

/// The editor opens with the whole text selected, so the first key replaces it; Enter commits
/// the edit once and closes the editor.
#[test]
fn a_double_click_opens_the_editor_and_enter_commits() {
    let id = WidgetId::from_hash("label-commit");
    let mut h = harness();
    let first = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(first, Shown::default());
    open(&mut h, id, true);
    h.key(Key::Char('X'));
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.key(Key::Char('Y'));
    let typed = h.frame_value(|ui| record(ui, id, true));
    assert!(typed.editing && typed.committed_text.is_none(), "{typed:?}");
    h.key(Key::Enter);
    let committed = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(committed.committed_text.as_deref(), Some("XY"));
    let after = h.frame_value(|ui| record(ui, id, true));
    assert!(
        !after.editing && after.committed_text.is_none(),
        "{after:?}"
    );
    assert_eq!(h.ui().focus(), None, "the closed editor let the focus go");
}

/// A caller opens the editor without a double click, as a Rename command does: the next frame
/// records it focused with the text selected, so a key replaces "Alpha", and Enter commits and
/// closes it. A label that is not editable stays a label.
#[test]
fn a_caller_opens_the_editor() {
    let id = WidgetId::from_hash("label-edit");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    assert!(!EditableLabel::is_editing(h.ui_ref(), id));
    EditableLabel::edit(h.ui(), id);
    assert!(EditableLabel::is_editing(h.ui_ref(), id));
    assert!(h.frame_value(|ui| record(ui, id, true)).editing);
    assert_eq!(h.ui().focus(), Some(id));
    h.key(Key::Char('Z'));
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.key(Key::Enter);
    let committed = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(committed.committed_text.as_deref(), Some("Z"));
    assert!(!EditableLabel::is_editing(h.ui_ref(), id));

    EditableLabel::edit(h.ui(), id);
    assert!(!h.frame_value(|ui| record(ui, id, false)).editing);
    assert!(!EditableLabel::is_editing(h.ui_ref(), id));
}

/// Escape closes the editor and commits nothing, and the next session starts from the text again.
#[test]
fn escape_keeps_the_text() {
    let id = WidgetId::from_hash("label-escape");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    open(&mut h, id, true);
    h.key(Key::Char('X'));
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.key(Key::Escape);
    let escaped = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(escaped.committed_text, None);
    assert!(!h.frame_value(|ui| record(ui, id, true)).editing);
    open(&mut h, id, true);
    h.key(Key::Enter);
    assert_eq!(
        h.frame_value(|ui| record(ui, id, true)).committed_text,
        None,
        "the new session starts at the text, so Enter changes nothing"
    );
}

/// A label that is not editable reports its clicks and opens nothing on a double click.
#[test]
fn a_label_that_is_not_editable_stays_a_label() {
    let id = WidgetId::from_hash("label-locked");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, false);
    });
    let hit = h.rect(id).expect("label arranged").center();
    h.click(hit);
    assert!(h.frame_value(|ui| record(ui, id, false)).clicked);
    h.click(hit);
    h.frame(|ui| {
        record(ui, id, false);
    });
    h.key(Key::Char('X'));
    h.key(Key::Enter);
    assert_eq!(h.frame_value(|ui| record(ui, id, false)), Shown::default());
    assert!(
        h.rect(EditableLabel::label_id(id)).is_some(),
        "the label stays a label"
    );
}

/// A drag that starts on the label reports its start, for a caller that moves what the label
/// names.
#[test]
fn a_drag_on_the_label_reports_its_start() {
    let id = WidgetId::from_hash("label-drag");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    let hit = h.rect(id).expect("label arranged").center();
    h.press(hit, PointerButton::Left);
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.drag_to(hit + Vec2::new(40.0, 0.0));
    assert!(h.frame_value(|ui| record(ui, id, true)).drag_started);
}
