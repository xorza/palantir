use crate::FocusPolicy;
use crate::KeyFilter;
use crate::cascade::Cascade;
use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::input_state::tests::{
    BUTTON_SURFACE, Sample, Stream, fixed_button, forged_focus, sample_layers,
};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::scroll_targets::ScrollTargets;
use crate::input::shortcut::Shortcut;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::visibility::Visibility;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
#[test]
fn keyboard_events_do_not_perturb_scroll_state() {
    let mut state = InputState::default();
    let target = WidgetId::from_hash("scroll");
    state.scroll_targets = ScrollTargets::both(target);
    state.feed(InputEvent::ScrollPixels(glam::Vec2::new(3.0, 5.0)));
    let before_scroll = state.frame_target_deltas.clone();
    state.feed(InputEvent::key_down(Key::ArrowLeft));
    state.feed(InputEvent::ModifiersChanged(Modifiers::NONE));
    assert_eq!(state.frame_target_deltas, before_scroll);
}

#[test]
fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
    let mut state = InputState::default();
    state.set_focus(Some(forged_focus()));

    state.feed(InputEvent::ModifiersChanged(Modifiers::CTRL));
    state.feed(InputEvent::key_down(Key::Char('a')));
    state.feed(InputEvent::ModifiersChanged(Modifiers::NONE));
    state.feed(InputEvent::KeyDown {
        key: Key::Char('b'),
        repeat: true,
        physical: Key::Other,
        text: KeyText::from_char('b'),
    });

    let presses = &state.frame_keyboard_events;
    assert_eq!(presses.len(), 2);
    assert_eq!(presses[0].key, Key::Char('a'));
    assert!(presses[0].mods.ctrl);
    assert!(!presses[0].repeat);
    assert_eq!(presses[1].key, Key::Char('b'));
    assert!(!presses[1].mods.ctrl);
    assert!(presses[1].repeat);
}

/// An app that declares **no scope at all** reads every chord: routing a chord to
/// "the scope the reader speaks for" would silence `key_pressed` for every consumer
/// that never calls `input_scope`. Both sides of the grant answer `None`, and `None
/// == None` keeps it working.
#[test]
fn a_tree_with_no_scopes_still_reads_every_chord() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    let bare = |ui: &mut Ui| {
        Block::new()
            .id(WidgetId::from_hash("plain"))
            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
            .show(ui);
    };
    h.frame(bare);
    press_escape(&mut h);
    let pressed = h.frame_value(|ui| {
        bare(ui);
        ui.key_pressed(Shortcut::key(Key::Escape))
    });
    assert!(pressed, "no scopes declared must not gate the chord out");
}

/// A scope silences the layers **strictly below** it and only those: an overlay
/// with one on `Layer::Popup` cuts `Main` off while its own layer and everything
/// above keep reading.
#[test]
fn a_scope_silences_the_layers_strictly_below_it() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    // The scope is declared during the first record; its path resolves from the
    // cascade at the start of the next.
    h.frame(popup_with_scope);
    press_escape(&mut h);
    let seen = sample_layers(&mut h, Stream::Keyboard, popup_with_scope).layers;

    assert_eq!(seen[Layer::Popup.idx()], 1, "the scope's own layer reads");
    assert_eq!(seen[Layer::Main.idx()], 0);
    assert_eq!(seen[Layer::Modal.idx()], 1);
    assert_eq!(seen[Layer::Tooltip.idx()], 1);
}

/// A scope that stops being recorded stops owning input, with no release call.
#[test]
fn a_scope_that_stops_recording_reopens_the_stream() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    h.frame(popup_with_scope);
    press_escape(&mut h);
    assert_eq!(
        sample_layers(&mut h, Stream::Keyboard, popup_with_scope).layers[Layer::Main.idx()],
        0
    );

    h.frame(|_| {});
    press_escape(&mut h);
    assert_eq!(
        sample_layers(&mut h, Stream::Keyboard, |_| {}).layers[Layer::Main.idx()],
        1
    );
}

/// A layer's fallback grant is its **outermost** scope, not its last-recorded one:
/// scopes record in pre-order, so granting the nested one would hand an app root's
/// accelerators to a text field inside it and kill them mid-edit.
#[test]
fn the_layer_fallback_grant_is_the_outermost_scope() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    let nested = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(60.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                let at_root = ui.key_pressed(Shortcut::key(Key::Escape));
                let at_inner = Panel::vstack()
                    .id(WidgetId::from_hash("inner"))
                    .input_scope(KeyFilter::ALL)
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui, |ui| ui.key_pressed(Shortcut::key(Key::Escape)))
                    .inner;
                [at_root, at_inner]
            })
            .inner
    };
    h.frame(|ui| {
        nested(ui);
    });
    press_escape(&mut h);
    let [at_root, at_inner] = h.frame_value(nested);

    assert!(
        at_root,
        "the outermost scope owns the layer's fallback grant"
    );
    assert!(!at_inner, "the nested scope must not shadow its container");
}

/// One overlay closing must not unblock a layer another still holds, and the
/// survivor must keep *reading*. Both orders run, since only one catches a scan
/// reading the stale cascade raw: closing `first` leaves the grant on `second`,
/// which passes by luck; closing `second` leaves the grant on the gone scope and
/// `first` reads nothing.
#[test]
fn closing_one_of_two_scopes_on_a_layer_leaves_it_blocked() {
    for closed in ["first", "second"] {
        let survivor = if closed == "first" { "second" } else { "first" };
        let mut h = UiHarness::new(glam::UVec2::new(200, 200));
        let two = |ui: &mut Ui| {
            let mut read_by_survivor = false;
            for id in ["first", "second"] {
                scope_leaf(ui, Layer::Popup, id, |ui| {
                    if id == survivor {
                        read_by_survivor = ui.key_pressed(Shortcut::key(Key::Escape));
                    }
                });
            }
            read_by_survivor
        };
        h.frame(|ui| {
            two(ui);
        });
        press_escape(&mut h);
        h.frame(|ui| {
            two(ui);
            ui.release_input_scope(WidgetId::from_hash(closed));
        });

        press_escape(&mut h);
        let Sample {
            layers: seen,
            value: read_by_survivor,
        } = sample_layers(&mut h, Stream::Keyboard, two);
        assert_eq!(
            seen[Layer::Main.idx()],
            0,
            "closing {closed}: the surviving scope keeps the layer blocked",
        );
        assert!(
            read_by_survivor,
            "closing {closed}: {survivor} must own the chord its closed sibling held",
        );
    }
}

/// A `release_input_scope` takes effect at the next resolution, like a focus move:
/// reads after it in the same pass get the answer reads before it got. Nested
/// scopes `root` around `inner`, with a read at `root` and two inside `inner`
/// either side of the release. With no focus anchor the grant falls to `root`; with
/// focus inside `inner` it is `inner`. After the release `inner` is withdrawn,
/// `root` holds the grant and every read lands.
#[test]
fn a_close_takes_effect_at_the_next_resolution() {
    #[derive(Debug, Default)]
    struct Reads {
        at_root: bool,
        inner_before: bool,
        inner_after: bool,
    }
    let nested = |ui: &mut Ui, focus_inside: bool, closes: bool| {
        let mut reads = Reads::default();
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(60.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                reads.at_root = ui.key_pressed(Shortcut::key(Key::Escape));
                Panel::vstack()
                    .id(WidgetId::from_hash("inner"))
                    .input_scope(KeyFilter::ALL)
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui, |ui| {
                        if focus_inside {
                            Block::new()
                                .id(WidgetId::from_hash("editor"))
                                .size(10.0)
                                .show(ui);
                        }
                        reads.inner_before = ui.key_pressed(Shortcut::key(Key::Escape));
                        if closes {
                            ui.release_input_scope(WidgetId::from_hash("inner"));
                        }
                        reads.inner_after = ui.key_pressed(Shortcut::key(Key::Escape));
                    });
            });
        reads
    };
    for focus_inside in [false, true] {
        let mut h = UiHarness::new(glam::UVec2::new(200, 200));
        // `closes` is false on the setup frame: a close outlives its frame.
        h.frame(|ui| {
            nested(ui, focus_inside, false);
        });
        press_escape(&mut h);
        let closing = h.frame_value(|ui| nested(ui, focus_inside, true));
        assert_eq!(
            (closing.inner_before, closing.at_root),
            (focus_inside, !focus_inside),
            "focus inside inner = {focus_inside}: the grant goes to inner only when focus anchors it",
        );
        assert_eq!(
            closing.inner_after, closing.inner_before,
            "focus inside inner = {focus_inside}: a read after the release in the same pass \
             must get the answer the read before it got",
        );
        press_escape(&mut h);
        let next = h.frame_value(|ui| nested(ui, focus_inside, false));
        assert!(
            next.at_root && next.inner_before && next.inner_after,
            "focus inside inner = {focus_inside}: on the next frame inner is withdrawn, \
             so root holds the grant and every read lands",
        );
    }
}

/// A steady frame keeps last frame's routing; a focus move, a new scope or a
/// withdrawal each resolve again once, a frame that only moves rects does not.
#[test]
fn scopes_resolve_again_only_when_their_inputs_change() {
    let scene = |ui: &mut Ui, extra: bool, width: f32, closes: bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(width), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("editor"))
                    .size(10.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("other"))
                    .size(10.0)
                    .show(ui);
                if extra {
                    Panel::vstack()
                        .id(WidgetId::from_hash("extra"))
                        .input_scope(KeyFilter::ALL)
                        .size(10.0)
                        .show(ui, |_| {});
                }
                if closes {
                    ui.release_input_scope(WidgetId::from_hash("root"));
                }
            });
    };
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    for _ in 0..3 {
        h.frame(|ui| scene(ui, false, 60.0, false));
    }
    let rebuilds = |h: &UiHarness| h.ui.input().scopes.rebuilds();
    let mut last = rebuilds(&h);
    let mut step = |h: &mut UiHarness, label: &str, want: u32, extra: bool, closes: bool| {
        h.frame(|ui| scene(ui, extra, 80.0, closes));
        let now = rebuilds(h);
        assert_eq!(now - last, want, "{label}");
        last = now;
    };
    step(&mut h, "rects only", 0, false, false);
    step(&mut h, "steady", 0, false, false);
    h.set_focus(WidgetId::from_hash("editor"));
    step(&mut h, "focus moved", 1, false, false);
    step(&mut h, "steady after the move", 0, false, false);
    step(&mut h, "a scope recorded, read next frame", 0, true, false);
    step(&mut h, "the cascade holds the new scope", 1, true, false);
    step(&mut h, "a withdrawal, read next frame", 0, true, true);
    step(&mut h, "the withdrawal resolved", 1, true, false);
}

/// Feed an Escape the keyboard wake-gate will deliver, focused on the fixture's
/// `editor` block: the gate drops an unsubscribed chord with nothing focused and
/// `end_frame` evicts focus on unrecorded widgets.
fn press_escape(h: &mut UiHarness) {
    h.set_focus(WidgetId::from_hash("editor"));
    h.key(Key::Escape);
}

/// A node on `layer` declaring an all-taking scope; `body` records inside it.
fn scope_leaf(ui: &mut Ui, layer: Layer, id: &'static str, body: impl FnOnce(&mut Ui)) {
    ui.layer(layer).show(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash(id))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
            .show(ui, body);
    });
}

fn popup_with_scope(ui: &mut Ui) {
    scope_leaf(ui, Layer::Popup, "overlay", |_| {});
}

#[test]
fn focus_policy_routing() {
    let cases: &[(&str, FocusPolicy, bool)] = &[
        ("preserve_keeps_focus", FocusPolicy::PreserveOnMiss, true),
        ("clear_drops_focus", FocusPolicy::ClearOnMiss, false),
    ];
    let editable_id = WidgetId::from_hash("editable");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(WidgetId::from_hash("editable"))
                .focusable(true)
                .show(ui);
        });
    };
    for (label, policy, expect_focus) in cases {
        let mut h = UiHarness::new(BUTTON_SURFACE);
        h.ui.set_focus_policy(*policy);
        h.frame(build);
        h.click_on(editable_id);
        assert_eq!(h.focus(), Some(editable_id), "{label}: initial focus");

        h.frame(build);
        let miss = glam::Vec2::new(180.0, 5.0);
        assert_eq!(h.hit_at(miss), None, "{label}: the press misses everything");
        h.press_at(miss);
        h.release();
        let expected = if *expect_focus {
            Some(editable_id)
        } else {
            None
        };
        assert_eq!(h.focus(), expected, "{label}: after outside press");
    }
    assert_eq!(
        UiHarness::new(BUTTON_SURFACE).ui.focus_policy(),
        FocusPolicy::ClearOnMiss
    );
}

#[test]
fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
    let surface = glam::UVec2::new(400, 80);
    let mut h = UiHarness::new(surface);
    h.ui.set_focus_policy(FocusPolicy::PreserveOnMiss);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(WidgetId::from_hash("editable"))
                .focusable(true)
                .show(ui);
            fixed_button(WidgetId::from_hash("plain"))
                .focusable(false)
                .show(ui);
        });
    };
    h.frame(build);
    h.click_on(WidgetId::from_hash("editable"));
    assert_eq!(h.focus(), Some(WidgetId::from_hash("editable")));

    h.frame(build);
    h.click_on(WidgetId::from_hash("plain"));
    assert_eq!(
        h.focus(),
        Some(WidgetId::from_hash("editable")),
        "click on non-focusable widget must not steal focus",
    );
}

#[test]
fn focus_is_evicted_when_widget_disappears() {
    let mut h = UiHarness::new(BUTTON_SURFACE);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(WidgetId::from_hash("editable"))
                .focusable(true)
                .show(ui);
        });
    });
    h.click_at(glam::Vec2::new(50.0, 20.0));
    assert_eq!(h.focus(), Some(WidgetId::from_hash("editable")));

    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |_ui| {});
    });
    assert_eq!(
        h.focus(),
        None,
        "focused widget removed from tree must drop focus",
    );
}

#[test]
fn set_focus_bypasses_policy() {
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let id = WidgetId::from_hash("manual");
    h.set_focus(id);
    assert_eq!(h.focus(), Some(id));
    h.clear_focus();
    assert_eq!(h.focus(), None);
}

#[test]
fn invisible_or_disabled_focusable_refuses_focus() {
    // Cascade combines `disabled` or `invisible`; pin both axes so a split cannot
    // keep one alive.

    #[derive(Debug)]
    enum Mode {
        Shown,
        Hidden,
        Disabled,
    }
    let editable = WidgetId::from_hash("editable");
    let cases: &[(&str, Mode, Option<WidgetId>)] = &[
        ("shown", Mode::Shown, Some(editable)),
        ("hidden", Mode::Hidden, None),
        ("disabled", Mode::Disabled, None),
    ];
    for (label, mode, expected) in cases {
        let mut h = UiHarness::new(BUTTON_SURFACE);
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let b = fixed_button(WidgetId::from_hash("editable")).focusable(true);
                match mode {
                    Mode::Shown => b.show(ui),
                    Mode::Hidden => b.visibility(Visibility::Hidden).show(ui),
                    Mode::Disabled => b.disabled(true).show(ui),
                };
            });
        });
        h.click_at(glam::Vec2::new(50.0, 20.0));
        assert_eq!(h.focus(), *expected, "case {label}");
    }
}

#[test]
fn post_record_clears_keys_but_preserves_modifiers() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    state.set_focus(Some(forged_focus()));
    state.feed(InputEvent::ModifiersChanged(Modifiers::SHIFT));
    state.feed(InputEvent::key_down(Key::ArrowLeft));
    let buf_cap_before = state.frame_keyboard_events.capacity();

    state.end_frame(&cascade);

    assert!(state.frame_keyboard_events.is_empty());
    assert_eq!(state.frame_keyboard_events.capacity(), buf_cap_before);
    assert!(state.modifiers.shift);
}
