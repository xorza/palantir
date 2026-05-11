use crate::input::keyboard::{Key, Modifiers, TextChunk, key_from_winit};
use crate::input::{InputEvent, InputState};
use crate::ui::cascade::CascadeResult;
use winit::event::WindowEvent;
use winit::keyboard::{Key as WK, NamedKey};

// `winit::event::KeyEvent` carries a platform_specific field that's
// non-portable to construct in tests, so we exercise the translation
// helper `key_from_winit` directly. The KeyboardInput→KeyDown
// wrapping in `from_winit` is a one-line `match event.state` — small
// enough that integration coverage of it can wait for a manual
// smoke-test in the showcase.

#[test]
fn key_from_winit_named_arrows() {
    assert_eq!(
        key_from_winit(&WK::Named(NamedKey::ArrowLeft)),
        Key::ArrowLeft
    );
    assert_eq!(
        key_from_winit(&WK::Named(NamedKey::ArrowRight)),
        Key::ArrowRight
    );
    assert_eq!(key_from_winit(&WK::Named(NamedKey::ArrowUp)), Key::ArrowUp);
    assert_eq!(
        key_from_winit(&WK::Named(NamedKey::ArrowDown)),
        Key::ArrowDown
    );
}

#[test]
fn key_from_winit_editing_keys() {
    assert_eq!(
        key_from_winit(&WK::Named(NamedKey::Backspace)),
        Key::Backspace
    );
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Delete)), Key::Delete);
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Home)), Key::Home);
    assert_eq!(key_from_winit(&WK::Named(NamedKey::End)), Key::End);
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Enter)), Key::Enter);
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Escape)), Key::Escape);
}

#[test]
fn key_from_winit_character_carries_first_char() {
    // Shift+'a' arrives as Character("A") post-layout — should keep
    // the capitalized form.
    assert_eq!(key_from_winit(&WK::Character("A".into())), Key::Char('A'));
    assert_eq!(key_from_winit(&WK::Character("é".into())), Key::Char('é'));
}

#[test]
fn key_from_winit_unknown_key_falls_back_to_other() {
    // `F24` exists in NamedKey but isn't enumerated in our `Key` —
    // should land in the catch-all rather than dropping the event.
    assert_eq!(key_from_winit(&WK::Named(NamedKey::F24)), Key::Other);
}

#[test]
fn key_from_winit_paging_navigation_keys() {
    assert_eq!(key_from_winit(&WK::Named(NamedKey::PageUp)), Key::PageUp);
    assert_eq!(
        key_from_winit(&WK::Named(NamedKey::PageDown)),
        Key::PageDown
    );
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Tab)), Key::Tab);
    // Space collapses to `Char(' ')` so the editor treats it as
    // ordinary text input — no dedicated variant.
    assert_eq!(key_from_winit(&WK::Named(NamedKey::Space)), Key::Char(' '));
}

#[test]
fn modifiers_from_winit_translates_each_bit() {
    use crate::input::keyboard::modifiers_from_winit;
    use winit::keyboard::ModifiersState;

    // Every flag off → all-default Modifiers.
    let m = modifiers_from_winit(&ModifiersState::empty());
    assert_eq!(m, Modifiers::NONE);

    // Each individual bit maps to the matching field.
    let m = modifiers_from_winit(&ModifiersState::SHIFT);
    assert!(m.shift && !m.ctrl && !m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::CONTROL);
    assert!(!m.shift && m.ctrl && !m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::ALT);
    assert!(!m.shift && !m.ctrl && m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::SUPER);
    assert!(!m.shift && !m.ctrl && !m.alt && m.meta);

    // Combined: shift+meta should set both.
    let m = modifiers_from_winit(&(ModifiersState::SHIFT | ModifiersState::SUPER));
    assert!(m.shift && m.meta && !m.ctrl && !m.alt);
}

#[test]
fn from_winit_ime_commit_emits_text_event() {
    let ev = InputEvent::from_winit(
        &WindowEvent::Ime(winit::event::Ime::Commit("é".into())),
        1.0,
    )
    .expect("Ime::Commit produces a Text event");
    match ev {
        InputEvent::Text(chunk) => assert_eq!(chunk.as_str(), "é"),
        _ => panic!("expected Text, got {ev:?}"),
    }
}

#[test]
fn from_winit_ime_commit_too_long_drops_event() {
    let long = "0123456789abcdef"; // 16 bytes — over inline cap
    let ev = InputEvent::from_winit(
        &WindowEvent::Ime(winit::event::Ime::Commit(long.into())),
        1.0,
    );
    assert!(
        ev.is_none(),
        "oversized IME commit drops cleanly rather than truncating"
    );
}

#[test]
fn keyboard_events_do_not_perturb_scroll_state() {
    // Pin: keyboard plumbing is independent of pointer/scroll. Scroll
    // delta accumulator must stay untouched even as keys, text, and
    // modifier changes flow in.
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    let before_scroll = state.frame_scroll_delta;
    state.on_input(
        InputEvent::KeyDown {
            key: Key::ArrowLeft,
            repeat: false,
        },
        &cascades,
    );
    state.on_input(InputEvent::Text(TextChunk::new("a").unwrap()), &cascades);
    state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
    assert_eq!(state.frame_scroll_delta, before_scroll);
}

#[test]
fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
    // Modifiers are captured at push time, not drain time, so a
    // ModifiersChanged event that lands between two KeyDowns
    // attributes correctly.
    let mut state = InputState::new();
    let cascades = CascadeResult::default();

    state.on_input(
        InputEvent::ModifiersChanged(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }),
        &cascades,
    );
    state.on_input(
        InputEvent::KeyDown {
            key: Key::Char('a'),
            repeat: false,
        },
        &cascades,
    );
    state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
    state.on_input(
        InputEvent::KeyDown {
            key: Key::Char('b'),
            repeat: true,
        },
        &cascades,
    );

    assert_eq!(state.frame_keys.len(), 2);
    assert_eq!(state.frame_keys[0].key, Key::Char('a'));
    assert!(state.frame_keys[0].mods.ctrl);
    assert!(!state.frame_keys[0].repeat);
    assert_eq!(state.frame_keys[1].key, Key::Char('b'));
    assert!(!state.frame_keys[1].mods.ctrl);
    assert!(state.frame_keys[1].repeat);
}

#[test]
fn text_events_concatenate_into_frame_text() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(InputEvent::Text(TextChunk::new("hé").unwrap()), &cascades);
    state.on_input(InputEvent::Text(TextChunk::new("llo").unwrap()), &cascades);
    assert_eq!(state.frame_text, "héllo");
}

#[test]
fn focus_lands_on_press_over_focusable_widget_and_preserve_holds_it() {
    // A focusable Button (we abuse the Button widget by setting
    // .focusable(true) — TextEdit doesn't exist yet) takes focus
    // when pressed. Under PreserveOnMiss, pressing on empty
    // surface afterwards keeps focus.
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::input::PointerButton;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
    );

    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase(); // Press past the focusable rect.
    ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(180.0, 5.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert_eq!(
        ui.focused_id(),
        Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
        "PreserveOnMiss keeps focus when press lands off any focusable widget",
    );
}

#[test]
fn default_policy_is_clear_on_miss() {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::input::PointerButton;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    // Pin: a fresh Ui starts with FocusPolicy::ClearOnMiss
    // (click-outside-to-blur is the native-app convention).
    let mut ui = Ui::new();
    assert_eq!(ui.focus_policy(), crate::FocusPolicy::ClearOnMiss);

    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert!(ui.focused_id().is_some());

    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(180.0, 5.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert_eq!(
        ui.focused_id(),
        None,
        "default ClearOnMiss drops focus on a press past the focusable",
    );
}

#[test]
fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
    // Two widgets: one focusable, one only clickable. Under
    // PreserveOnMiss, clicking the pure-Click widget shouldn't
    // steal focus from the focusable one. (Under default
    // ClearOnMiss this isn't true — the press lands on a
    // non-focusable widget and clears focus.)
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    begin(&mut ui, glam::UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
    );

    begin(&mut ui, glam::UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    // Click the plain button — it captures the click but isn't
    // focusable, so focus stays on "editable".
    click_at(&mut ui, glam::Vec2::new(150.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
        "click on non-focusable widget must not steal focus",
    );
}

#[test]
fn focus_is_evicted_when_widget_disappears() {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert!(ui.focused_id().is_some());

    // Next frame omits the focusable widget entirely.
    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |_ui| {});
    ui.record_phase();
    ui.paint_phase();
    assert_eq!(
        ui.focused_id(),
        None,
        "focused widget removed from tree must drop focus",
    );
}

#[test]
fn request_focus_bypasses_policy() {
    use crate::Ui;
    let mut ui = Ui::new();
    let id = crate::forest::widget_id::WidgetId::from_hash("manual");
    ui.request_focus(Some(id));
    assert_eq!(ui.focused_id(), Some(id));
    ui.request_focus(None);
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn invisible_focusable_widget_does_not_take_focus() {
    // Cascade rule: invisible nodes drop their focusable bit just
    // like they drop their `Sense`. Pin separately from the
    // disabled case because the cascade combines `disabled ||
    // invisible` and a future split would silently keep one bit
    // alive.
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::visibility::Visibility;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .visibility(Visibility::Hidden)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        None,
        "invisible focusable widget refuses focus",
    );
}

#[test]
fn disabled_focusable_widget_does_not_take_focus() {
    // Cascade rule: disabled (or invisible) nodes drop their focusable
    // bit just like they drop their `Sense` — keystrokes shouldn't
    // route to a greyed-out field.
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{begin, click_at};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    begin(&mut ui, glam::UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new()
            .id_salt("editable")
            .focusable(true)
            .disabled(true)
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        None,
        "disabled focusable widget refuses focus",
    );
}

#[test]
fn end_frame_clears_keys_and_text_but_preserves_modifiers() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(
        InputEvent::ModifiersChanged(Modifiers {
            shift: true,
            ..Modifiers::NONE
        }),
        &cascades,
    );
    state.on_input(
        InputEvent::KeyDown {
            key: Key::ArrowLeft,
            repeat: false,
        },
        &cascades,
    );
    state.on_input(InputEvent::Text(TextChunk::new("x").unwrap()), &cascades);
    let key_cap_before = state.frame_keys.capacity();
    let text_cap_before = state.frame_text.capacity();

    state.end_frame(&cascades);

    assert!(state.frame_keys.is_empty());
    assert!(state.frame_text.is_empty());
    // Capacity-retained: typing across frames stays alloc-free in
    // steady state.
    assert_eq!(state.frame_keys.capacity(), key_cap_before);
    assert_eq!(state.frame_text.capacity(), text_cap_before);
    // Modifier state is a running snapshot, not per-frame — held
    // shift across frames must remain `true`.
    assert!(state.modifiers.shift);
}
