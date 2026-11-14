use crate::theme::Theme;
use glam::{UVec2, Vec2};

use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::editable_label::EditableLabel;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::edit_end::EditEnd;

/// What one frame of a label reported, from its last pass.
#[derive(Debug, Default, PartialEq)]
struct Shown {
    editing: bool,
    clicked: bool,
    drag_started: bool,
    committed: bool,
    committed_text: Option<String>,
    ended: Option<EditEnd>,
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
        committed: response.committed,
        committed_text: response.committed_text,
        ended: response.ended,
    }
}

/// A harness whose display does not snap, so rects compare exactly.
fn harness() -> Harness {
    let surface = UVec2::new(300, 100);
    Harness::builder(surface)
        .mono()
        .pixel_snap(false)
        .build()
        .with_fixture_font_size()
}

/// Double-click the label `id` and run the frame its editor opens in.
fn open(h: &mut Harness, id: WidgetId, editable: bool) {
    let hit = h.rect(id).expect("label arranged").center();
    h.double_click(hit);
    for _ in 0..2 {
        h.frame(|ui| {
            record(ui, id, editable);
        });
    }
}

/// The label holds its rect across the swap to its editor and back, for the default theme, with
/// its padding and border, for a flat one, for a padding the caller sets, and for a font size the
/// caller sets, which reaches the editor too; in the flat one the text sits on the label's leading
/// edge, where the editor draws it. An empty label is as high as one with text: the stock line,
/// 19.203125 px, floors both at its 20 px block, inside the padding.
#[test]
fn the_label_and_its_editor_take_one_rect() {
    let flat = {
        let mut theme = Theme::stock(ColorScheme::Dark).text_edit;
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
    let mut heights = Vec::new();
    for (name, style, padding, font_size) in [
        ("default", None, None, None),
        ("flat", Some(&flat), None, None),
        ("padded", None, Some(Spacing::new(9.0, 3.0, 5.0, 7.0)), None),
        ("large", None, None, Some(30.0)),
    ] {
        for text in ["Ab", ""] {
            let name = format!("{name} {text:?}");
            let id = WidgetId::from_hash(("label-rect", name.as_str()));
            let mut h = harness();
            let show = |ui: &mut Ui| {
                let mut label = EditableLabel::new(text).id(id).style(style);
                if let Some(padding) = padding {
                    label = label.padding(padding);
                }
                if let Some(px) = font_size {
                    label = label.font_size(px);
                }
                label.show(ui).editing
            };
            assert!(!h.frame_value(show));
            let idle = h.rect(id).expect("label arranged");
            heights.push(idle.size.h);
            if style.is_some() {
                let text = h.rect(id.with("label")).expect("text arranged");
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
    for [text, empty] in heights.as_chunks::<2>().0 {
        assert_eq!(empty, text, "an empty label is as high as one with text");
    }
    assert_eq!(heights[2], 20.0, "the flat label is its one line's block");
    assert!(heights[6] > heights[0], "the font size reached the label");
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
    assert!(!typed.committed && typed.ended.is_none(), "{typed:?}");
    h.key(Key::Enter);
    let committed = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(committed.committed_text.as_deref(), Some("XY"));
    assert!(committed.committed);
    assert_eq!(committed.ended, Some(EditEnd::Submitted));
    let after = h.frame_value(|ui| record(ui, id, true));
    assert!(
        !after.editing && after.committed_text.is_none(),
        "{after:?}"
    );
    assert_eq!(h.ui().focus(), None, "the closed editor let the focus go");
}

/// A caller opens the editor without a double click, as a Rename command does: the next frame
/// records it focused with the text selected, so a key replaces "Alpha", and Enter commits and
/// closes it. A label that is not editable or is disabled stays a label.
#[test]
fn a_caller_opens_the_editor() {
    let id = WidgetId::from_hash("label-edit");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    assert!(!EditableLabel::is_editing(h.ui(), id));
    EditableLabel::start_editing(h.ui_mut(), id);
    assert!(EditableLabel::is_editing(h.ui(), id));
    assert!(h.frame_value(|ui| record(ui, id, true)).editing);
    assert_eq!(h.ui().focus(), Some(id));
    h.key(Key::Char('Z'));
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.key(Key::Enter);
    let committed = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(committed.committed_text.as_deref(), Some("Z"));
    assert!(!EditableLabel::is_editing(h.ui(), id));

    // An editor whose opening focus another widget took first closes on its frame, with no
    // commit: no blur would come to close it.
    EditableLabel::start_editing(h.ui_mut(), id);
    h.ui_mut().set_focus(WidgetId::from_hash("label-elsewhere"));
    let unfocused = h.frame_value(|ui| record(ui, id, true));
    assert!(
        unfocused.editing && !unfocused.committed,
        "the frame it records"
    );
    assert!(
        !EditableLabel::is_editing(h.ui(), id),
        "the focus went elsewhere"
    );

    EditableLabel::start_editing(h.ui_mut(), id);
    assert!(!h.frame_value(|ui| record(ui, id, false)).editing);
    assert!(!EditableLabel::is_editing(h.ui(), id));
    assert_eq!(
        h.ui().focus(),
        None,
        "the closed editor gives its focus back"
    );

    // A disabled label stays a label too, and its keys reach no editor.
    EditableLabel::start_editing(h.ui_mut(), id);
    let disabled = |ui: &mut Ui| {
        EditableLabel::new("Alpha")
            .id(id)
            .disabled(true)
            .show(ui)
            .editing
    };
    assert!(!h.frame_value(disabled));
    h.key(Key::Char('Q'));
    h.key(Key::Enter);
    assert!(!h.frame_value(disabled));
    assert!(!EditableLabel::is_editing(h.ui(), id));
    assert_eq!(h.ui().focus(), None);

    // So does one a disabled ancestor holds; the ancestor is known from the frame before.
    let in_disabled = |ui: &mut Ui| {
        Panel::vstack()
            .auto_id()
            .disabled(true)
            .show(ui, |ui| EditableLabel::new("Alpha").id(id).show(ui).editing)
            .inner
    };
    h.frame(|ui| {
        in_disabled(ui);
    });
    EditableLabel::start_editing(h.ui_mut(), id);
    assert!(!h.frame_value(in_disabled));
    assert!(!EditableLabel::is_editing(h.ui(), id));
    assert_eq!(h.ui().focus(), None);

    // A focused label disabled on this very frame, before the cascade knows it, takes no F2.
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    h.ui_mut().set_focus(id);
    h.frame(|ui| {
        record(ui, id, true);
    });
    assert_eq!(h.ui().focus(), Some(id), "premise: the label holds focus");
    h.key(Key::F2);
    let opened = h.frame_passes(|ui| {
        disabled(ui);
        EditableLabel::is_editing(ui, id)
    });
    assert_eq!(
        opened.count_where(|opened| *opened),
        0,
        "no pass opens the editor"
    );
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
    assert!(!escaped.committed);
    assert_eq!(escaped.ended, Some(EditEnd::Canceled));
    assert!(!h.frame_value(|ui| record(ui, id, true)).editing);
    // Opened by the caller: a double click here would land as the third and fourth clicks of a
    // run, which open nothing.
    EditableLabel::start_editing(h.ui_mut(), id);
    assert!(h.frame_value(|ui| record(ui, id, true)).editing);
    h.key(Key::Enter);
    let unchanged = h.frame_value(|ui| record(ui, id, true));
    assert_eq!(
        unchanged.committed_text, None,
        "the new session starts at the text, so Enter changes nothing"
    );
    assert!(unchanged.committed, "and still commits");
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
        h.rect(id.with("label")).is_some(),
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

/// `stop_editing` ends an edit from outside: `Submitted` commits the draft, as a blur does, and
/// `Canceled` keeps the text, as Escape does; either way the next frame shows the label. Enter or
/// F2 on the focused label opens the editor.
#[test]
fn a_caller_ends_the_edit_and_the_keys_open_it() {
    let id = WidgetId::from_hash("label-stop");
    let mut h = harness();
    h.frame(|ui| {
        record(ui, id, true);
    });
    for (end, want) in [(EditEnd::Submitted, Some("Z")), (EditEnd::Canceled, None)] {
        EditableLabel::start_editing(h.ui_mut(), id);
        h.frame(|ui| {
            record(ui, id, true);
        });
        h.key(Key::Char('Z'));
        h.frame(|ui| {
            record(ui, id, true);
        });
        EditableLabel::stop_editing(h.ui_mut(), id, end);
        let ended = h.frame_value(|ui| record(ui, id, true));
        assert_eq!(ended.committed_text.as_deref(), want, "{end:?}");
        assert_eq!(ended.committed, want.is_some(), "{end:?}");
        assert!(!EditableLabel::is_editing(h.ui(), id), "{end:?}");
        assert!(!h.frame_value(|ui| record(ui, id, true)).editing, "{end:?}");
    }

    for key in [Key::Enter, Key::F2] {
        h.ui_mut().set_focus(id);
        h.frame(|ui| {
            record(ui, id, true);
        });
        h.key(key);
        h.frame(|ui| {
            record(ui, id, true);
        });
        assert!(EditableLabel::is_editing(h.ui(), id), "{key:?} opens it");
        EditableLabel::stop_editing(h.ui_mut(), id, EditEnd::Canceled);
        h.frame(|ui| {
            record(ui, id, true);
        });
    }
}
