use crate::input::keyboard::key_text::KeyText;
use crate::internals::harness::passes::Passes;
use crate::widgets::text_edit::internals::EditEdges;
use crate::widgets::text_edit::tests::*;

const EDITOR: &str = "response-editor";

/// Drives one frame and keeps each record pass's edges: `Ui::frame` re-records on relayout, so a per-frame edge
/// shows only in pass A.
fn frame(h: &mut UiHarness, buf: &mut String) -> Passes<EditEdges> {
    h.frame_passes(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(buf)
                    .id(WidgetId::from_hash(EDITOR))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .edges()
            })
            .inner
    })
}

#[test]
fn reports_focus_gained_as_a_one_frame_edge() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    assert!(
        !frame(&mut h, &mut buf).a().focus_gained,
        "unfocused: no gain"
    );
    h.set_focus(id);
    assert!(
        frame(&mut h, &mut buf).a().focus_gained,
        "took focus this frame"
    );
    assert!(
        !frame(&mut h, &mut buf).a().focus_gained,
        "gain clears after one frame"
    );
}

#[test]
fn reports_changed_on_edit_but_not_submit() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    h.key(Key::Char('x'));
    let s = frame(&mut h, &mut buf);
    assert_eq!(buf, "x");
    assert!(s.a().changed && !s.a().submitted, "an edit is not a submit");
}

#[test]
fn reports_submitted_on_single_line_enter() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("hi");

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    h.key(Key::Enter);
    let s = frame(&mut h, &mut buf);
    assert!(s.a().submitted, "single-line Enter submits");
    assert!(!s.a().changed, "Enter inserts nothing in single-line");
    assert_eq!(buf, "hi", "buffer untouched by the submit");
}

/// `committed` fires once per finished edit, against hand-written sequences; each step is one frame.
#[test]
fn committed_fires_once_per_finished_edit() {
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Type(char),
        Press(Key),
        Blur,
    }
    use Step::{Blur, Press, Type};
    let cases: [(&str, &[Step], &[bool]); 6] = [
        (
            "an edit commits on the blur",
            &[Type('x'), Blur, Type('y')],
            &[false, true, false],
        ),
        ("focus and blur with no edit commit", &[Blur], &[true]),
        ("Enter commits at once", &[Press(Key::Enter)], &[true]),
        (
            "a blur after Enter does not commit again",
            &[Press(Key::Enter), Blur],
            &[true, false],
        ),
        (
            "an edit after Enter commits again on the blur",
            &[Press(Key::Enter), Type('y'), Blur],
            &[true, false, true],
        ),
        (
            "Escape blurs and commits nothing",
            &[Type('x'), Press(Key::Escape), Blur],
            &[false, false, false],
        ),
    ];
    for (label, steps, want) in cases {
        let mut h = UiHarness::with_text(SMALL);
        let mut buf = String::new();
        h.set_focus(WidgetId::from_hash(EDITOR));
        assert!(
            !frame(&mut h, &mut buf).a().committed,
            "{label}: focus frame"
        );
        let got: Vec<bool> = steps
            .iter()
            .map(|step| {
                match *step {
                    Type(c) => drop(h.key(Key::Char(c))),
                    Press(key) => drop(h.key(key)),
                    Blur => h.clear_focus(),
                }
                frame(&mut h, &mut buf).a().committed
            })
            .collect();
        assert_eq!(got, want, "{label}");
    }
}

#[test]
fn reports_focus_lost_on_blur() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    h.clear_focus();
    assert!(
        frame(&mut h, &mut buf).a().focus_lost,
        "lost focus this frame"
    );
}

#[test]
fn escape_reports_focus_lost_on_the_blur_frame() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Escape);
    let escaped = frame(&mut h, &mut buf);
    assert!(
        escaped.a().focus_lost,
        "Escape reports the focus edge immediately"
    );
    assert!(h.focus().is_none());
    assert!(
        !frame(&mut h, &mut buf).a().focus_lost,
        "the edge is not repeated next frame",
    );
}

/// A same-length overwrite must still report `changed`: the signal comes from the mutation choke points, not a length delta.
#[test]
fn reports_changed_on_same_length_overwrite() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::from("a");

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    h.set_modifiers(Modifiers::CTRL);
    h.key(Key::Char('a'));
    h.set_modifiers(Modifiers::NONE);
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Char('b'));
    let sig = frame(&mut h, &mut buf);
    assert_eq!(buf, "b", "overwrite replaced the selection");
    assert!(sig.a().changed, "same-length overwrite reports changed");
}

/// Disabling a focused editor kicks focus out on the disable frame and drops that frame's keystrokes.
#[test]
fn disabling_a_focused_editor_blurs_and_drops_input() {
    fn disabled_frame(h: &mut UiHarness, buf: &mut String) -> Passes<EditEdges> {
        h.frame_passes(|ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash(EDITOR))
                        .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                        .disabled(true)
                        .show(ui)
                        .edges()
                })
                .inner
        })
    }

    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus on the enabled editor
    h.key(Key::Char('x'));
    let sig = disabled_frame(&mut h, &mut buf);
    assert_eq!(buf, "", "typing into a disabled editor is dropped");
    assert!(!sig.a().changed, "no change reported");
    assert!(sig.a().focus_lost, "disable frame reports focus_lost");
    assert!(!sig.a().committed, "a blur by disabling commits nothing");
    assert!(h.focus().is_none(), "focus was kicked out");
}

/// Every chord `TextEdit` binds as an editing action must classify as [`KeyClass::Edit`], or a focused editor
/// stops taking it and the app steals it mid-edit; pins the hand-kept `key_class::EDIT_CHORDS`.
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
        // `Shortcut` matches on the physical key, so classify the press as the router sees it.
        let press = KeyPress {
            key: shortcut.key,
            mods: Modifiers {
                ctrl: shortcut.mods.ctrl,
                shift: shortcut.mods.shift,
                alt: shortcut.mods.alt,
                mac_ctrl: false,
                meta: shortcut.mods.meta,
            },
            repeat: false,
            physical: shortcut.key,
            text: KeyText::EMPTY,
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

/// A focused field takes only the keys it acts on: arrows and Home/End move its caret, so an app root misses them;
/// Tab, Shift+Tab, page keys and Ctrl+Tab reach the root's own scope (which must declare one and take `FOCUS`).
#[test]
fn a_focused_field_yields_the_keys_it_does_not_act_on() {
    use crate::KeyFilter;
    use crate::input::shortcut::{Shortcut, ShortcutMods};

    let field = WidgetId::from_hash(EDITOR);
    let scene = |ui: &mut Ui, buf: &mut String, probe: Shortcut| {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL | KeyFilter::FOCUS)
            .show(ui, |ui| {
                let at_root = ui.key_pressed(probe);
                TextEdit::new(buf)
                    .id(field)
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui);
                at_root
            })
            .inner
    };
    let shift = Modifiers::SHIFT;
    let ctrl = Modifiers::CTRL;
    for (key, mods, probe_mods, reaches_root) in [
        (Key::ArrowLeft, Modifiers::NONE, ShortcutMods::NONE, false),
        (Key::End, Modifiers::NONE, ShortcutMods::NONE, false),
        (Key::Tab, Modifiers::NONE, ShortcutMods::NONE, true),
        (Key::Tab, shift, ShortcutMods::SHIFT, true),
        (Key::PageDown, Modifiers::NONE, ShortcutMods::NONE, true),
        (Key::Tab, ctrl, ShortcutMods::CTRL, true),
    ] {
        let probe = Shortcut::new(probe_mods, key);
        let mut h = UiHarness::with_text(SMALL);
        let mut buf = String::from("hello");
        h.frame(|ui| {
            scene(ui, &mut buf, probe);
        });
        h.set_focus(field);
        // Settles the scope path, which resolves against the previous frame's cascade.
        h.frame(|ui| {
            scene(ui, &mut buf, probe);
        });
        h.set_modifiers(mods);
        h.key(key);
        let at_root = h.frame_value(|ui| scene(ui, &mut buf, probe));
        assert_eq!(at_root, reaches_root, "{key:?} under {mods:?}");
        assert_eq!(h.focus(), Some(field), "{key:?}: the field keeps focus");
    }
}
