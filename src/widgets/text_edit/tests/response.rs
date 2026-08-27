use crate::ui::harness::UiHarness;
use crate::widgets::text_edit::tests::*;

/// The edit signals off one `TextEdit::show`, snapshotted out (the response
/// itself borrows `ui`, so it can't escape the frame closure).
#[derive(Debug, Default, Clone, Copy)]
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
fn frame(h: &mut UiHarness, buf: &mut String) -> Signals {
    let mut out = Signals::default();
    h.frame(|ui| {
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
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    assert!(!frame(&mut h, &mut buf).gained, "unfocused: no gain");
    h.request_focus(Some(id));
    assert!(frame(&mut h, &mut buf).gained, "took focus this frame");
    assert!(
        !frame(&mut h, &mut buf).gained,
        "gain clears after one frame"
    );
}

#[test]
fn reports_changed_on_edit_but_not_submit() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf); // settle focus
    h.key(Key::Char('x'));
    let s = frame(&mut h, &mut buf);
    assert_eq!(buf, "x");
    assert!(s.changed && !s.submitted, "an edit is not a submit");
}

#[test]
fn reports_submitted_on_single_line_enter() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("hi");

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf); // settle focus
    h.key(Key::Enter);
    let s = frame(&mut h, &mut buf);
    assert!(s.submitted, "single-line Enter submits");
    assert!(!s.changed, "Enter inserts nothing in single-line");
    assert_eq!(buf, "hi", "buffer untouched by the submit");
}

#[test]
fn reports_lost_focus_on_blur() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf); // settle focus
    h.request_focus(None);
    assert!(frame(&mut h, &mut buf).lost, "lost focus this frame");
}

#[test]
fn escape_reports_lost_focus_on_the_blur_frame() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Escape);
    let escaped = frame(&mut h, &mut buf);
    assert!(escaped.lost, "Escape reports the focus edge immediately");
    assert!(h.focused_id().is_none());
    assert!(
        !frame(&mut h, &mut buf).lost,
        "the edge is not repeated next frame",
    );
}

/// A same-length overwrite (select the buffer, type a replacement) must
/// still report `changed` — the signal comes from the mutation choke
/// points, not a length delta ("a" → "b" keeps len 1).
#[test]
fn reports_changed_on_same_length_overwrite() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("a");

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf); // settle focus
    // Ctrl+A select-all, then type the replacement.
    h.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    h.key(Key::Char('a'));
    h.set_modifiers(Modifiers::NONE);
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Char('b'));
    let sig = frame(&mut h, &mut buf);
    assert_eq!(buf, "b", "overwrite replaced the selection");
    assert!(sig.changed, "same-length overwrite reports changed");
}

/// Disabling a focused editor kicks focus out on the disable frame
/// (`lost_focus` fires) and the same frame's keystrokes are dropped —
/// behavior agrees with the disabled visuals instead of silently
/// routing typing into the host's buffer.
#[test]
fn disabling_a_focused_editor_blurs_and_drops_input() {
    fn disabled_frame(h: &mut UiHarness, buf: &mut String) -> Signals {
        let mut out = Signals::default();
        h.frame(|ui| {
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

    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.request_focus(Some(id));
    let _ = frame(&mut h, &mut buf); // settle focus on the enabled editor
    h.key(Key::Char('x'));
    let sig = disabled_frame(&mut h, &mut buf);
    assert_eq!(buf, "", "typing into a disabled editor is dropped");
    assert!(!sig.changed, "no change reported");
    assert!(sig.lost, "disable frame reports lost_focus");
    assert!(h.focused_id().is_none(), "focus was kicked out");
}

/// Every chord `TextEdit` binds as an editing action must classify as
/// [`KeyClass::Edit`], or a focused editor stops taking it and the app
/// steals it mid-edit.
///
/// The pin behind `key_class::EDIT_CHORDS`, which is a hand-kept list
/// living one crate-module away from this one. A seventh `EditAction`
/// that forgets to extend it fails here rather than silently becoming an
/// accelerator.
#[test]
fn every_edit_action_chord_is_edit_class() {
    use crate::KeyClass;
    use crate::input::keyboard::key_press::KeyPress;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::widgets::text_edit::action::EditAction;

    let actions = [
        EditAction::Undo,
        EditAction::Redo,
        EditAction::SelectAll,
        EditAction::Cut,
        EditAction::Copy,
        EditAction::Paste,
        EditAction::Clear,
    ];
    let mut checked = 0;
    for action in actions {
        let Some(shortcut) = action.shortcut() else {
            continue;
        };
        // `Shortcut` matches on the physical key, so classify the press
        // the same way the router will see it.
        let press = KeyPress {
            key: shortcut.key,
            mods: Modifiers {
                ctrl: shortcut.mods.ctrl,
                shift: shortcut.mods.shift,
                alt: shortcut.mods.alt,
                mac_ctrl: false,
            },
            repeat: false,
            physical: shortcut.key,
        };
        assert_eq!(
            KeyClass::of(press),
            KeyClass::Edit,
            "{action:?} binds {shortcut:?}, which a focused field must take",
        );
        checked += 1;
    }
    assert_eq!(checked, 6, "six of the seven actions carry a chord");
}
