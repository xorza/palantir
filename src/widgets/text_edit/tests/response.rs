use crate::input::keyboard::key_text::KeyText;
use crate::internals::harness::passes::Passes;
use crate::widgets::text_edit::internals::EditEdges;
use crate::widgets::text_edit::tests::*;

const EDITOR: &str = "response-editor";

/// Drive one frame and keep each record pass's edges. `Ui::frame`
/// re-records on relayout, and the second pass sees a drained input
/// queue — the *buffer* survives (it's cross-frame state), but a
/// per-frame edge only shows in pass A.
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
fn reports_gained_focus_as_a_one_frame_edge() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    assert!(
        !frame(&mut h, &mut buf).a().gained_focus,
        "unfocused: no gain"
    );
    h.set_focus(id);
    assert!(
        frame(&mut h, &mut buf).a().gained_focus,
        "took focus this frame"
    );
    assert!(
        !frame(&mut h, &mut buf).a().gained_focus,
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

#[test]
fn reports_lost_focus_on_blur() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    h.clear_focus();
    assert!(
        frame(&mut h, &mut buf).a().lost_focus,
        "lost focus this frame"
    );
}

#[test]
fn escape_reports_lost_focus_on_the_blur_frame() {
    let mut h = UiHarness::with_text(SMALL);
    let id = WidgetId::from_hash(EDITOR);
    let mut buf = String::new();

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Escape);
    let escaped = frame(&mut h, &mut buf);
    assert!(
        escaped.a().lost_focus,
        "Escape reports the focus edge immediately"
    );
    assert!(h.focused_id().is_none());
    assert!(
        !frame(&mut h, &mut buf).a().lost_focus,
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

    h.set_focus(id);
    let _ = frame(&mut h, &mut buf); // settle focus
    // Ctrl+A select-all, then type the replacement.
    h.set_modifiers(Modifiers::CTRL);
    h.key(Key::Char('a'));
    h.set_modifiers(Modifiers::NONE);
    let _ = frame(&mut h, &mut buf);
    h.key(Key::Char('b'));
    let sig = frame(&mut h, &mut buf);
    assert_eq!(buf, "b", "overwrite replaced the selection");
    assert!(sig.a().changed, "same-length overwrite reports changed");
}

/// Disabling a focused editor kicks focus out on the disable frame
/// (`lost_focus` fires) and the same frame's keystrokes are dropped —
/// behavior agrees with the disabled visuals instead of silently
/// routing typing into the host's buffer.
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
    assert!(sig.a().lost_focus, "disable frame reports lost_focus");
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

/// A focused field takes only the keys it acts on. The arrows and Home /
/// End move its caret, so an app root reading them misses; Tab, Shift+Tab,
/// the page keys and Ctrl+Tab do nothing in a field, so they reach the
/// root's own scope. The root declares one — without it, the root would
/// read as the layer's outermost scope, the field's, and see every key.
#[test]
fn a_focused_field_yields_the_keys_it_does_not_act_on() {
    use crate::KeyFilter;
    use crate::input::shortcut::{Shortcut, ShortcutMods};

    let field = WidgetId::from_hash(EDITOR);
    let scene = |ui: &mut Ui, buf: &mut String, probe: Shortcut| {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL)
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
        // Settles the scope path, which resolves against the previous
        // frame's cascade.
        h.frame(|ui| {
            scene(ui, &mut buf, probe);
        });
        h.set_modifiers(mods);
        h.key(key);
        let at_root = h.frame_value(|ui| scene(ui, &mut buf, probe));
        assert_eq!(at_root, reaches_root, "{key:?} under {mods:?}");
        assert_eq!(
            h.focused_id(),
            Some(field),
            "{key:?}: the field keeps focus"
        );
    }
}
