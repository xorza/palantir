use crate::input::keyboard::{Key, Modifiers, TextChunk, key_from_winit};
use crate::input::{InputEvent, InputState};
use crate::ui::cascade::Cascades;
use winit::event::WindowEvent;
use winit::keyboard::{Key as WK, NamedKey};

#[test]
fn key_from_winit_named_keys() {
    use NamedKey::*;
    let cases: &[(NamedKey, Key)] = &[
        (ArrowLeft, Key::ArrowLeft),
        (ArrowRight, Key::ArrowRight),
        (ArrowUp, Key::ArrowUp),
        (ArrowDown, Key::ArrowDown),
        (Backspace, Key::Backspace),
        (Delete, Key::Delete),
        (Home, Key::Home),
        (End, Key::End),
        (Enter, Key::Enter),
        (Escape, Key::Escape),
        (PageUp, Key::PageUp),
        (PageDown, Key::PageDown),
        (Tab, Key::Tab),
        // Space collapses to Char(' ') so the editor treats it as text input.
        (Space, Key::Char(' ')),
        // Unenumerated NamedKeys land in the catch-all rather than dropping.
        (F24, Key::Other),
    ];
    for (named, expected) in cases {
        assert_eq!(key_from_winit(&WK::Named(*named)), *expected);
    }
}

#[test]
fn key_from_winit_character_carries_first_char() {
    assert_eq!(key_from_winit(&WK::Character("A".into())), Key::Char('A'));
    assert_eq!(key_from_winit(&WK::Character("é".into())), Key::Char('é'));
}

#[test]
fn modifiers_from_winit_translates_each_bit() {
    use crate::input::keyboard::modifiers_from_winit;
    use winit::keyboard::ModifiersState;

    let m = modifiers_from_winit(&ModifiersState::empty());
    assert_eq!(m, Modifiers::NONE);

    let m = modifiers_from_winit(&ModifiersState::SHIFT);
    assert!(m.shift && !m.ctrl && !m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::CONTROL);
    assert!(!m.shift && m.ctrl && !m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::ALT);
    assert!(!m.shift && !m.ctrl && m.alt && !m.meta);
    let m = modifiers_from_winit(&ModifiersState::SUPER);
    assert!(!m.shift && !m.ctrl && !m.alt && m.meta);

    let m = modifiers_from_winit(&(ModifiersState::SHIFT | ModifiersState::SUPER));
    assert!(m.shift && m.meta && !m.ctrl && !m.alt);
}

#[test]
fn from_winit_ime_commit_routing() {
    // (label, payload, expect_text). None expect = dropped cleanly.
    // (label, payload). Long commits split at char boundaries — the
    // concatenated chunks must roundtrip back to the original.
    let cases: &[(&str, &str)] = &[
        ("short_grapheme_emits_text", "é"),
        ("over_inline_cap_splits", "0123456789abcdef"),
        ("cjk_long_commit_splits", "日本語入力テスト文字列"),
    ];
    for (label, s) in cases {
        let mut got = String::new();
        InputEvent::from_winit(
            &WindowEvent::Ime(winit::event::Ime::Commit((*s).into())),
            1.0,
            |ev| match ev {
                InputEvent::Text(chunk) => got.push_str(chunk.as_str()),
                other => panic!("case {label}: unexpected {other:?}"),
            },
        );
        assert_eq!(got, *s, "case {label}: roundtrip");
    }
}

#[test]
fn keyboard_events_do_not_perturb_scroll_state() {
    let mut state = InputState::new();
    let cascades = Cascades::default();
    let before_scroll = state.frame_scroll_pixels;
    state.on_input(
        InputEvent::KeyDown {
            key: Key::ArrowLeft,
            repeat: false,
        },
        &cascades,
    );
    state.on_input(InputEvent::Text(TextChunk::new("a").unwrap()), &cascades);
    state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
    assert_eq!(state.frame_scroll_pixels, before_scroll);
}

#[test]
fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
    // Modifiers captured at push time, so a ModifiersChanged between
    // two KeyDowns attributes correctly.
    let mut state = InputState::new();
    let cascades = Cascades::default();

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
    let cascades = Cascades::default();
    state.on_input(InputEvent::Text(TextChunk::new("hé").unwrap()), &cascades);
    state.on_input(InputEvent::Text(TextChunk::new("llo").unwrap()), &cascades);
    assert_eq!(state.frame_text, "héllo");
}

#[test]
fn focus_policy_routing() {
    use crate::FocusPolicy;
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::widget_id::WidgetId;
    use crate::input::PointerButton;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{click_at, run_at_acked};
    use crate::widgets::{button::Button, panel::Panel};

    // (label, policy, expect_focus_after_outside_press).
    let cases: &[(&str, FocusPolicy, bool)] = &[
        ("preserve_keeps_focus", FocusPolicy::PreserveOnMiss, true),
        ("clear_drops_focus", FocusPolicy::ClearOnMiss, false),
    ];
    let surface = glam::UVec2::new(200, 80);
    let editable_id = WidgetId::from_hash("editable");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    };
    for (label, policy, expect_focus) in cases {
        let mut ui = Ui::new();
        ui.set_focus_policy(*policy);
        run_at_acked(&mut ui, surface, build);
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(ui.focused_id(), Some(editable_id), "{label}: initial focus");

        run_at_acked(&mut ui, surface, build);
        ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(180.0, 5.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        let expected = if *expect_focus {
            Some(editable_id)
        } else {
            None
        };
        assert_eq!(ui.focused_id(), expected, "{label}: after outside press");
    }
    // Default policy is ClearOnMiss.
    assert_eq!(Ui::new().focus_policy(), FocusPolicy::ClearOnMiss);
}

#[test]
fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::widget_id::WidgetId;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{click_at, run_at_acked};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    let surface = glam::UVec2::new(400, 80);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
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
    };
    run_at_acked(&mut ui, surface, build);
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editable")));

    run_at_acked(&mut ui, surface, build);
    click_at(&mut ui, glam::Vec2::new(150.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        Some(WidgetId::from_hash("editable")),
        "click on non-focusable widget must not steal focus",
    );
}

#[test]
fn focus_is_evicted_when_widget_disappears() {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{click_at, run_at_acked};
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::new();
    let surface = glam::UVec2::new(200, 80);
    run_at_acked(&mut ui, surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    assert!(ui.focused_id().is_some());

    run_at_acked(&mut ui, surface, |ui| {
        Panel::hstack().auto_id().show(ui, |_ui| {});
    });
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
fn invisible_or_disabled_focusable_refuses_focus() {
    // Cascade combines `disabled || invisible`; pin both axes so a
    // future split doesn't keep one alive.
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::visibility::Visibility;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::{click_at, run_at_acked};
    use crate::widgets::{button::Button, panel::Panel};

    enum Mode {
        Hidden,
        Disabled,
    }
    let cases: &[(&str, Mode)] = &[("hidden", Mode::Hidden), ("disabled", Mode::Disabled)];
    for (label, mode) in cases {
        let mut ui = Ui::new();
        run_at_acked(&mut ui, glam::UVec2::new(200, 80), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let b = Button::new()
                    .id_salt("editable")
                    .focusable(true)
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)));
                match mode {
                    Mode::Hidden => b.visibility(Visibility::Hidden).show(ui),
                    Mode::Disabled => b.disabled(true).show(ui),
                };
            });
        });
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(ui.focused_id(), None, "case {label}");
    }
}

#[test]
fn post_record_clears_keys_and_text_but_preserves_modifiers() {
    let mut state = InputState::new();
    let cascades = Cascades::default();
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

    state.post_record(&cascades);

    assert!(state.frame_keys.is_empty());
    assert!(state.frame_text.is_empty());
    // Capacity-retained: typing across frames stays alloc-free.
    assert_eq!(state.frame_keys.capacity(), key_cap_before);
    assert_eq!(state.frame_text.capacity(), text_cap_before);
    assert!(state.modifiers.shift);
}
