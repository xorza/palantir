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
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
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
