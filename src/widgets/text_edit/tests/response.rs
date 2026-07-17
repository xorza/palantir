use crate::widgets::text_edit::tests::*;

/// The edit signals off one `TextEdit::show`, snapshotted out (the response
/// itself borrows `ui`, so it can't escape the frame closure).
#[derive(Default, Clone, Copy)]
struct Signals {
    changed: bool,
    submitted: bool,
    gained: bool,
    lost: bool,
}

const EDITOR: &str = "response-editor";

/// Drive one frame and OR-accumulate the response signals across its record
/// passes. `Ui::frame` re-records on relayout, and the second pass sees a
/// drained input queue — the *buffer* survives (it's cross-frame state) but a
/// per-frame edge signal would read `false` on the second pass, so combine them.
fn frame(ui: &mut Ui, buf: &mut String) -> Signals {
    let mut out = Signals::default();
    ui.run_at_acked(SMALL, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let r = TextEdit::new(buf)
                .id(WidgetId::from_hash(EDITOR))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
            out.changed |= r.changed;
            out.submitted |= r.submitted;
            out.gained |= r.gained_focus;
            out.lost |= r.lost_focus;
        });
    });
    out
}

#[test]
fn reports_gained_focus_as_a_one_frame_edge() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    assert!(!frame(&mut ui, &mut buf).gained, "unfocused: no gain");
    ui.request_focus(Some(id));
    assert!(frame(&mut ui, &mut buf).gained, "took focus this frame");
    assert!(
        !frame(&mut ui, &mut buf).gained,
        "gain clears after one frame"
    );
}

#[test]
fn reports_changed_on_edit_but_not_submit() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    ui.request_focus(Some(id));
    let _ = frame(&mut ui, &mut buf); // settle focus
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
        physical: Key::Other,
    });
    let s = frame(&mut ui, &mut buf);
    assert_eq!(buf, "x");
    assert!(s.changed && !s.submitted, "an edit is not a submit");
}

#[test]
fn reports_submitted_on_single_line_enter() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("hi");

    ui.request_focus(Some(id));
    let _ = frame(&mut ui, &mut buf); // settle focus
    ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
        physical: Key::Other,
    });
    let s = frame(&mut ui, &mut buf);
    assert!(s.submitted, "single-line Enter submits");
    assert!(!s.changed, "Enter inserts nothing in single-line");
    assert_eq!(buf, "hi", "buffer untouched by the submit");
}

#[test]
fn reports_lost_focus_on_blur() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    ui.request_focus(Some(id));
    let _ = frame(&mut ui, &mut buf); // settle focus
    ui.request_focus(None);
    assert!(frame(&mut ui, &mut buf).lost, "lost focus this frame");
}

/// A same-length overwrite (select the buffer, type a replacement) must
/// still report `changed` — the signal comes from the mutation choke
/// points, not a length delta ("a" → "b" keeps len 1).
#[test]
fn reports_changed_on_same_length_overwrite() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("a");

    ui.request_focus(Some(id));
    let _ = frame(&mut ui, &mut buf); // settle focus
    // Ctrl+A select-all, then type the replacement.
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('a'),
        repeat: false,
        physical: Key::Other,
    });
    ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
    let _ = frame(&mut ui, &mut buf);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('b'),
        repeat: false,
        physical: Key::Other,
    });
    let sig = frame(&mut ui, &mut buf);
    assert_eq!(buf, "b", "overwrite replaced the selection");
    assert!(sig.changed, "same-length overwrite reports changed");
}

/// Disabling a focused editor kicks focus out on the disable frame
/// (`lost_focus` fires) and the same frame's keystrokes are dropped —
/// behavior agrees with the disabled visuals instead of silently
/// routing typing into the host's buffer.
#[test]
fn disabling_a_focused_editor_blurs_and_drops_input() {
    fn disabled_frame(ui: &mut Ui, buf: &mut String) -> Signals {
        let mut out = Signals::default();
        ui.run_at_acked(SMALL, |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let r = TextEdit::new(buf)
                    .id(WidgetId::from_hash(EDITOR))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .disabled(true)
                    .show(ui);
                out.changed |= r.changed;
                out.lost |= r.lost_focus;
            });
        });
        out
    }

    let mut ui = Ui::for_test_at_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    ui.request_focus(Some(id));
    let _ = frame(&mut ui, &mut buf); // settle focus on the enabled editor
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
        physical: Key::Other,
    });
    let sig = disabled_frame(&mut ui, &mut buf);
    assert_eq!(buf, "", "typing into a disabled editor is dropped");
    assert!(!sig.changed, "no change reported");
    assert!(sig.lost, "disable frame reports lost_focus");
    assert!(ui.focused_id().is_none(), "focus was kicked out");
}
