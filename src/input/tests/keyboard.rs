use crate::FocusPolicy;
use crate::KeyFilter;
use crate::input::keyboard::{Key, KeyboardEvent, Modifiers, TextChunk};
use crate::input::{InputEvent, InputState};
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascades;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use strum::EnumCount as _;
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

/// A scope silences the layers **strictly below** it, and only those.
///
/// The property the old whole-stream claim carried, now a consequence of
/// where a scope sits: an overlay declaring one on `Layer::Popup` cuts
/// `Main` off, while its own layer and everything above keep reading —
/// which is what lets a `TextEdit` inside the popup go on draining.
#[test]
fn a_scope_silences_the_layers_strictly_below_it() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    // Something focused, so the keyboard wake-gate delivers an
    // unsubscribed chord at all. Not a recorded id, so it anchors no
    // scope path — these cases are about the layer gate.
    // Two frames: the scope is declared during the first record and the
    // path resolves from the cascade at the start of the next.
    h.frame(popup_with_scope);
    press_escape(&mut h);
    let seen = sample_layers(&mut h, popup_with_scope);

    assert_eq!(seen[Layer::Popup.idx()], 1, "the scope's own layer reads");
    // Strictly below — cut off, which is the whole point.
    assert_eq!(seen[Layer::Main.idx()], 0);
    // Above — a modal over a popup is not silenced by it, the case that
    // once left a modal unable to see its own Escape.
    assert_eq!(seen[Layer::Modal.idx()], 1);
    assert_eq!(seen[Layer::Tooltip.idx()], 1);
}

/// A scope that stops being recorded stops owning input — no release
/// call, and no frame of ownership after the overlay is gone.
#[test]
fn a_scope_that_stops_recording_reopens_the_stream() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    h.frame(popup_with_scope);
    press_escape(&mut h);
    assert_eq!(
        sample_layers(&mut h, popup_with_scope)[Layer::Main.idx()],
        0
    );

    // Popup gone: its scope leaves the cascade, so the next resolution
    // hands `Main` the stream back.
    h.frame(|_| {});
    press_escape(&mut h);
    assert_eq!(sample_layers(&mut h, |_| {})[Layer::Main.idx()], 1);
}

/// One overlay closing must not unblock a layer another still holds.
/// Per-scope, not per-layer — the property `release` used to carry.
#[test]
fn closing_one_of_two_scopes_on_a_layer_leaves_it_blocked() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    let two = |ui: &mut Ui| {
        scope_leaf(ui, Layer::Popup, "first");
        scope_leaf(ui, Layer::Popup, "second");
    };
    h.frame(two);
    press_escape(&mut h);
    h.frame(|ui| {
        two(ui);
        ui.close_scope(WidgetId::from_hash("first"));
    });

    // The next resolution honours the close, and `second` still holds.
    press_escape(&mut h);
    assert_eq!(
        sample_layers(&mut h, two)[Layer::Main.idx()],
        0,
        "the surviving scope keeps the layer blocked",
    );
}

/// Feed an Escape that the keyboard wake-gate will actually deliver.
/// The gate drops an unsubscribed chord when nothing is focused, and
/// `end_frame` evicts focus whose widget was not recorded — so this has
/// to be set immediately before the press, not once up front.
fn press_escape(h: &mut UiHarness) {
    h.ui.input.focused = Some(WidgetId::from_hash("editor"));
    h.key(Key::Escape);
}

/// Per-layer keyboard-event counts read *inside* the record — the only
/// place they are live, since `end_frame` drains the queue. Maxed across
/// passes: action input makes `Ui::frame` record twice and the second
/// pass sees a drained queue.
fn sample_layers(h: &mut UiHarness, mut record: impl FnMut(&mut Ui)) -> [usize; Layer::COUNT] {
    let mut seen = [0usize; Layer::COUNT];
    h.frame(|ui| {
        record(ui);
        for layer in Layer::PAINT_ORDER {
            let n = ui.input.keyboard_events(layer).len();
            seen[layer.idx()] = seen[layer.idx()].max(n);
        }
    });
    seen
}

/// A leaf on `layer` declaring an all-taking scope — the shape every
/// overlay reduces to once `modal_layer` became `input_scope`.
fn scope_leaf(ui: &mut Ui, layer: Layer, id: &'static str) {
    ui.layer(layer, glam::Vec2::ZERO, None, |ui| {
        Frame::new()
            .id(WidgetId::from_hash(id))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
            .show(ui);
    });
}

fn popup_with_scope(ui: &mut Ui) {
    scope_leaf(ui, Layer::Popup, "overlay");
}

#[test]
fn focus_policy_routing() {
    use crate::FocusPolicy;
    use crate::Ui;
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
        let mut h = UiHarness::new(surface);
        h.ui.set_focus_policy(*policy);
        h.frame(build);
        h.click_at(glam::Vec2::new(50.0, 20.0));
        assert_eq!(h.focused_id(), Some(editable_id), "{label}: initial focus");

        h.frame(build);
        h.press_at(glam::Vec2::new(180.0, 5.0));
        h.release();
        let expected = if *expect_focus {
            Some(editable_id)
        } else {
            None
        };
        assert_eq!(h.focused_id(), expected, "{label}: after outside press");
    }
    // Default policy is ClearOnMiss.
    assert_eq!(
        UiHarness::new(surface).ui.focus_policy(),
        FocusPolicy::ClearOnMiss
    );
}

#[test]
fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::node::Configure;
    use crate::widgets::{button::Button, panel::Panel};

    let surface = glam::UVec2::new(400, 80);
    let mut h = UiHarness::new(surface);
    h.ui.set_focus_policy(FocusPolicy::PreserveOnMiss);
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
    h.frame(build);
    h.click_at(glam::Vec2::new(50.0, 20.0));
    assert_eq!(h.focused_id(), Some(WidgetId::from_hash("editable")));

    h.frame(build);
    h.click_at(glam::Vec2::new(150.0, 20.0));
    assert_eq!(
        h.focused_id(),
        Some(WidgetId::from_hash("editable")),
        "click on non-focusable widget must not steal focus",
    );
}

#[test]
fn focus_is_evicted_when_widget_disappears() {
    use crate::layout::types::sizing::Sizing;
    use crate::scene::node::Configure;
    use crate::widgets::{button::Button, panel::Panel};

    let surface = glam::UVec2::new(200, 80);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("editable"))
                .focusable(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    });
    h.click_at(glam::Vec2::new(50.0, 20.0));
    assert!(h.focused_id().is_some());

    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |_ui| {});
    });
    assert_eq!(
        h.focused_id(),
        None,
        "focused widget removed from tree must drop focus",
    );
}

#[test]
fn request_focus_bypasses_policy() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 80));
    let id = WidgetId::from_hash("manual");
    h.request_focus(Some(id));
    assert_eq!(h.focused_id(), Some(id));
    h.request_focus(None);
    assert_eq!(h.focused_id(), None);
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
        let mut h = UiHarness::new(glam::UVec2::new(200, 80));
        h.frame(|ui| {
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
        h.click_at(glam::Vec2::new(50.0, 20.0));
        assert_eq!(h.focused_id(), None, "case {label}");
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
