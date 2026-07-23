use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::{InputEvent, InputState};
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascades;
use crate::{FocusPolicy, Ui};
#[test]
fn keyboard_events_do_not_perturb_scroll_state() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    let target = WidgetId::from_hash("scroll");
    state.scroll_target = Some(target);
    state.on_input(
        InputEvent::ScrollPixels(glam::Vec2::new(3.0, 5.0)),
        &cascades,
    );
    let before_scroll = state.frame_target_deltas.clone();
    state.on_input(
        InputEvent::KeyDown {
            key: Key::ArrowLeft,
            repeat: false,
            physical: Key::Other,
        },
        &cascades,
    );
    state.on_input(InputEvent::Text(TextChunk::new("a").unwrap()), &cascades);
    state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
    assert_eq!(state.frame_target_deltas, before_scroll);
}

#[test]
fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
    // Modifiers captured at push time, so a ModifiersChanged between
    // two KeyDowns attributes correctly.
    let mut state = InputState::default();
    let cascades = Cascades::default();
    state.focused = Some(WidgetId::from_hash("editor"));

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
            physical: Key::Other,
        },
        &cascades,
    );
    state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
    state.on_input(
        InputEvent::KeyDown {
            key: Key::Char('b'),
            repeat: true,
            physical: Key::Other,
        },
        &cascades,
    );

    use crate::input::keyboard::KeyboardEvent;
    let presses: Vec<_> = state
        .frame_keyboard_events
        .iter()
        .filter_map(|e| match e {
            KeyboardEvent::Down(kp) => Some(*kp),
            _ => None,
        })
        .collect();
    assert_eq!(presses.len(), 2);
    assert_eq!(presses[0].key, Key::Char('a'));
    assert!(presses[0].mods.ctrl);
    assert!(!presses[0].repeat);
    assert_eq!(presses[1].key, Key::Char('b'));
    assert!(!presses[1].mods.ctrl);
    assert!(presses[1].repeat);
}

#[test]
fn text_events_arrive_in_order_in_keyboard_buffer() {
    use crate::input::keyboard::KeyboardEvent;
    let mut state = InputState::default();
    let cascades = Cascades::default();
    state.focused = Some(WidgetId::from_hash("editor"));
    state.on_input(InputEvent::Text(TextChunk::new("hé").unwrap()), &cascades);
    state.on_input(InputEvent::Text(TextChunk::new("llo").unwrap()), &cascades);
    let texts: Vec<_> = state
        .frame_keyboard_events
        .iter()
        .filter_map(|e| match e {
            KeyboardEvent::Text(c) => Some(c.as_str().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["hé".to_string(), "llo".to_string()]);
}

#[test]
fn focus_policy_routing() {
    use crate::FocusPolicy;
    use crate::Ui;
    use crate::input::pointer::PointerButton;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::node::Configure;
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
                .id(WidgetId::from_hash("editable"))
                .focusable(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    for (label, policy, expect_focus) in cases {
        let mut ui = Ui::for_test();
        ui.set_focus_policy(*policy);
        ui.run_at(surface, build);
        ui.click_at(glam::Vec2::new(50.0, 20.0));
        assert_eq!(ui.focused_id(), Some(editable_id), "{label}: initial focus");

        ui.run_at(surface, build);
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
    assert_eq!(Ui::for_test().focus_policy(), FocusPolicy::ClearOnMiss);
}

#[test]
fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::node::Configure;
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::for_test();
    ui.set_focus_policy(FocusPolicy::PreserveOnMiss);
    let surface = glam::UVec2::new(400, 80);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("editable"))
                .focusable(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
            Button::new()
                .id(WidgetId::from_hash("plain"))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    ui.run_at(surface, build);
    ui.click_at(glam::Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editable")));

    ui.run_at(surface, build);
    ui.click_at(glam::Vec2::new(150.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        Some(WidgetId::from_hash("editable")),
        "click on non-focusable widget must not steal focus",
    );
}

#[test]
fn focus_is_evicted_when_widget_disappears() {
    use crate::layout::types::sizing::Sizing;
    use crate::scene::node::Configure;
    use crate::widgets::{button::Button, panel::Panel};

    let mut ui = Ui::for_test();
    let surface = glam::UVec2::new(200, 80);
    ui.run_at(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("editable"))
                .focusable(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    });
    ui.click_at(glam::Vec2::new(50.0, 20.0));
    assert!(ui.focused_id().is_some());

    ui.run_at(surface, |ui| {
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
    let mut ui = Ui::for_test();
    let id = WidgetId::from_hash("manual");
    ui.request_focus(Some(id));
    assert_eq!(ui.focused_id(), Some(id));
    ui.request_focus(None);
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn invisible_or_disabled_focusable_refuses_focus() {
    // Cascade combines `disabled || invisible`; pin both axes so a
    // future split doesn't keep one alive.

    use crate::layout::types::sizing::Sizing;
    use crate::scene::node::Configure;
    use crate::scene::visibility::Visibility;
    use crate::widgets::{button::Button, panel::Panel};

    enum Mode {
        Hidden,
        Disabled,
    }
    let cases: &[(&str, Mode)] = &[("hidden", Mode::Hidden), ("disabled", Mode::Disabled)];
    for (label, mode) in cases {
        let mut ui = Ui::for_test();
        ui.run_at(glam::UVec2::new(200, 80), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let b = Button::new()
                    .id(WidgetId::from_hash("editable"))
                    .focusable(true)
                    .size((Sizing::fixed(100.0), Sizing::fixed(40.0)));
                match mode {
                    Mode::Hidden => b.visibility(Visibility::Hidden).show(ui),
                    Mode::Disabled => b.disabled(true).show(ui),
                };
            });
        });
        ui.click_at(glam::Vec2::new(50.0, 20.0));
        assert_eq!(ui.focused_id(), None, "case {label}");
    }
}

#[test]
fn post_record_clears_keys_and_text_but_preserves_modifiers() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    state.focused = Some(WidgetId::from_hash("editor"));
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
            physical: Key::Other,
        },
        &cascades,
    );
    state.on_input(InputEvent::Text(TextChunk::new("x").unwrap()), &cascades);
    let buf_cap_before = state.frame_keyboard_events.capacity();

    state.end_frame(&cascades);

    assert!(state.frame_keyboard_events.is_empty());
    // Capacity-retained: typing across frames stays alloc-free.
    assert_eq!(state.frame_keyboard_events.capacity(), buf_cap_before);
    assert!(state.modifiers.shift);
}
