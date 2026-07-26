use crate::input::keyboard::{Key, KeyboardEvent, Modifiers, TextChunk};
use crate::input::shortcut::Shortcut;
use crate::input::{InputEvent, InputState};
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascades;
use crate::scene::layer::Layer;
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
fn keyboard_views_and_shortcuts_follow_capture_owner() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    state.focused = Some(WidgetId::from_hash("editor"));
    state.on_input(
        InputEvent::KeyDown {
            key: Key::Escape,
            repeat: false,
            physical: Key::Escape,
        },
        &cascades,
    );
    let shortcut = Shortcut::key(Key::Escape);
    let event = state.keyboard_events(Layer::Main)[0];
    let KeyboardEvent::Down(keypress) = event else {
        panic!("expected queued key press");
    };

    assert_eq!(state.keyboard_events(Layer::Main), &[event]);
    assert!(state.key_pressed(Layer::Main, shortcut));
    assert!(state.subs.matches_press(keypress));
    assert_eq!(state.subs.keys.len(), 1);

    let owner = WidgetId::from_hash("popup");
    let other = WidgetId::from_hash("other-popup");
    state.claim_input(owner, Layer::Popup);
    state.finish_record();

    assert!(state.keyboard_events(Layer::Main).is_empty());
    assert!(!state.key_pressed(Layer::Main, shortcut));
    assert_eq!(state.claimed_keyboard_events(owner), &[event]);
    assert!(state.claimed_key_pressed(owner, shortcut));
    assert!(state.claimed_keyboard_events(other).is_empty());
    assert!(!state.claimed_key_pressed(other, shortcut));
    assert_eq!(state.subs.keys.len(), 1);

    // Capture is layer-*ordered*, not exclusive: it silences only readers
    // strictly *below* the capturing overlay's layer.
    //
    // Above — the `Modal` over `Popup` case that previously left a modal
    // unable to see its own Escape, and `Tooltip` above that.
    assert_eq!(state.keyboard_events(Layer::Modal), &[event]);
    assert!(state.key_pressed(Layer::Modal, shortcut));
    assert_eq!(state.keyboard_events(Layer::Tooltip), &[event]);
    // Same layer — the capturing overlay's own interior. `Popup` holds
    // capture across its whole body, so a non-capturing widget in there
    // (a `TextEdit`, which drains the uncaptured stream) has to keep
    // reading or it cannot be typed into.
    assert_eq!(state.keyboard_events(Layer::Popup), &[event]);
    // Strictly below — cut off, which is the whole point of capturing.
    assert!(state.keyboard_events(Layer::Main).is_empty());
    assert!(!state.key_pressed(Layer::Main, shortcut));
}

/// A modal layer's claim covers both streams and releases both at once.
/// Asserted together rather than in two tests because the point of
/// bundling them is that their lifecycles can no longer diverge — which
/// is exactly what a per-stream test would stop catching.
#[test]
fn a_modal_layer_claim_retains_or_releases_both_streams() {
    let mut ui = Ui::for_test();
    ui.input.focused = Some(WidgetId::from_hash("editor"));
    ui.watch_pointer(crate::input::watch::PointerWake::BUTTONS);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Escape,
    });
    ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(5.0, 5.0)));
    ui.on_input(InputEvent::PointerPressed(
        crate::input::pointer::PointerButton::Left,
    ));
    let key = ui.keyboard_events()[0];
    let press = ui.pointer_events()[0];
    let owner = WidgetId::from_hash("popup");
    let shortcut = Shortcut::key(Key::Escape);

    // The claim records the popup layer, and the handle outlives the
    // scope — the point of handing it to the body by value.
    let claim = ui.modal_layer(Layer::Popup, glam::Vec2::ZERO, None, owner, |_, claim| {
        claim
    });
    // Nothing moves until the pass resolves — for *either* stream. The
    // two used to disagree here: the keyboard half took effect on the
    // claiming pass and the pointer half on the next one, because
    // claiming eagerly wrote the committed keyboard owner when none was
    // held. One list resolved in one place is what removed that.
    assert_eq!(
        ui.keyboard_events(),
        &[key],
        "the claiming pass reads as if unclaimed",
    );
    assert_eq!(ui.pointer_events(), &[press], "and so does the pointer");

    ui.input.finish_record();
    assert!(ui.keyboard_events().is_empty(), "Main is below the claim");
    assert!(
        ui.pointer_events().is_empty(),
        "the same claim gates the pointer stream",
    );
    assert_eq!(claim.keyboard_events(&ui), &[key]);
    assert!(claim.key_pressed(&mut ui, shortcut));
    // The trap the handle exists to close: out here the ambient layer is
    // `Main`, so an owner reading `ui.pointer_events()` is silenced by
    // its *own* claim. Reading through the claim sees the layer it holds.
    assert_eq!(claim.pointer_events(&ui), &[press]);

    // Releasing is symmetric with claiming: it withdraws from the *next*
    // resolution, not from the pass it is called in.
    ui.input.begin_record();
    let claim = ui.modal_layer(Layer::Popup, glam::Vec2::ZERO, None, owner, |_, claim| {
        claim
    });
    claim.release(&mut ui);
    assert!(
        ui.keyboard_events().is_empty(),
        "the released pass keeps the ownership it was committed with",
    );
    ui.input.finish_record();
    assert_eq!(ui.keyboard_events(), &[key]);
    assert_eq!(ui.pointer_events(), &[press]);
}

/// Two overlays can hold one layer at once, so releasing is per-claim
/// and not per-layer: the first to close must not unblock the layer
/// while the second is still up.
#[test]
fn releasing_one_of_two_claims_on_a_layer_leaves_it_blocked() {
    let mut ui = Ui::for_test();
    ui.watch_pointer(crate::input::watch::PointerWake::BUTTONS);
    ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(5.0, 5.0)));
    ui.on_input(InputEvent::PointerPressed(
        crate::input::pointer::PointerButton::Left,
    ));
    let press = ui.pointer_events()[0];

    let first = ui.modal_layer(
        Layer::Popup,
        glam::Vec2::ZERO,
        None,
        WidgetId::from_hash("first"),
        |_, claim| claim,
    );
    let second = ui.modal_layer(
        Layer::Popup,
        glam::Vec2::ZERO,
        None,
        WidgetId::from_hash("second"),
        |_, claim| claim,
    );

    first.release(&mut ui);
    ui.input.finish_record();
    assert!(
        ui.pointer_events().is_empty(),
        "the second popup still holds Layer::Popup",
    );

    ui.input.begin_record();
    let second = ui.modal_layer(
        Layer::Popup,
        glam::Vec2::ZERO,
        None,
        WidgetId::from_hash("second"),
        |_, _| second,
    );
    second.release(&mut ui);
    ui.input.finish_record();
    assert_eq!(
        ui.pointer_events(),
        &[press],
        "the last claim leaving unblocks the layer",
    );
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
