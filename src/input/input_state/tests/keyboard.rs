use crate::FocusPolicy;
use crate::KeyFilter;
use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::input_state::tests::{
    BUTTON_SURFACE, Sample, Stream, fixed_button, forged_focus, sample_layers,
};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::scene::layer::Layer;
use crate::scene::visibility::Visibility;
use crate::ui::Ui;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
#[test]
fn keyboard_events_do_not_perturb_scroll_state() {
    let mut state = InputState::default();
    let target = WidgetId::from_hash("scroll");
    state.scroll_target = Some(target);
    state.feed(InputEvent::ScrollPixels(glam::Vec2::new(3.0, 5.0)));
    let before_scroll = state.frame_target_deltas.clone();
    state.feed(InputEvent::key_down(Key::ArrowLeft));
    state.feed(InputEvent::ModifiersChanged(Modifiers::NONE));
    assert_eq!(state.frame_target_deltas, before_scroll);
}

#[test]
fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
    // Modifiers captured at push time, so a ModifiersChanged between
    // two KeyDowns attributes correctly.
    let mut state = InputState::default();
    state.set_focus(Some(forged_focus()));

    state.feed(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
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

/// An app that declares **no scope at all** reads every chord.
///
/// The regression scopes invite: routing a chord to "the scope the reader
/// speaks for" silences the reader outright when there is no scope to
/// speak for, which would leave `key_pressed` dead for every consumer
/// that never calls `input_scope` — the showcase, the examples, any host
/// that only wants accelerators. Both sides of the grant answer `None`
/// there, and `None == None` is what keeps it working.
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
        ui.escape_pressed()
    });
    assert!(pressed, "no scopes declared must not gate the chord out");
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
    let seen = sample_layers(&mut h, Stream::Keyboard, popup_with_scope).layers;

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
        sample_layers(&mut h, Stream::Keyboard, popup_with_scope).layers[Layer::Main.idx()],
        0
    );

    // Popup gone: its scope leaves the cascade, so the next resolution
    // hands `Main` the stream back.
    h.frame(|_| {});
    press_escape(&mut h);
    assert_eq!(
        sample_layers(&mut h, Stream::Keyboard, |_| {}).layers[Layer::Main.idx()],
        1
    );
}

/// A layer's fallback grant is its **outermost** scope, not its
/// last-recorded one.
///
/// Scopes record in pre-order, so a scope nested inside another comes
/// last. Resolving the fallback to it would hand an app root's
/// accelerators to whatever text field happens to sit inside it, and
/// every one of them would die mid-edit — so the root reads the chord
/// and the nested scope, which the grant passed over, does not.
#[test]
fn the_layer_fallback_grant_is_the_outermost_scope() {
    let mut h = UiHarness::new(glam::UVec2::new(200, 200));
    // Whether Escape reads at the root and inside the inner scope.
    let nested = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(60.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                let at_root = ui.escape_pressed();
                let at_inner = Panel::vstack()
                    .id(WidgetId::from_hash("inner"))
                    .input_scope(KeyFilter::ALL)
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui, |ui| ui.escape_pressed())
                    .inner;
                [at_root, at_inner]
            })
            .inner
    };
    h.frame(|ui| {
        nested(ui);
    });
    press_escape(&mut h);
    // Focus sits on an unrecorded id, so no scope path anchors and the
    // grant falls through to the layer's outermost scope — the case this
    // is about.
    let [at_root, at_inner] = h.frame_value(nested);

    assert!(
        at_root,
        "the outermost scope owns the layer's fallback grant"
    );
    assert!(!at_inner, "the nested scope must not shadow its container");
}

/// One overlay closing must not unblock a layer another still holds,
/// and the survivor has to keep *reading* — either half alone is a
/// silent failure, so both are asserted per closed sibling.
///
/// Per-scope, not per-layer — the property `release` used to carry.
///
/// Both orders run, because only one of them catches a scan that reads
/// the stale cascade raw. Sibling scopes contain nobody, so the
/// outermost-of fold falls through to "last recorded": closing `first`
/// leaves the grant on `second`, which is also the survivor and passes
/// by luck. Closing `second` is the case that bites — the grant stays
/// assigned to the scope that is gone, `first` reads nothing, and no
/// layer count moves to show it.
#[test]
fn closing_one_of_two_scopes_on_a_layer_leaves_it_blocked() {
    for closed in ["first", "second"] {
        let survivor = if closed == "first" { "second" } else { "first" };
        let mut h = UiHarness::new(glam::UVec2::new(200, 200));
        // Read from inside the survivor, which is where a scoped chord
        // has to land now that its sibling is gone.
        let two = |ui: &mut Ui| {
            let mut read_by_survivor = false;
            for id in ["first", "second"] {
                scope_leaf(ui, Layer::Popup, id, |ui| {
                    if id == survivor {
                        read_by_survivor = ui.escape_pressed();
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

        // The next resolution honours the close, and the sibling holds.
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

/// A `release_input_scope` takes effect at the next resolution, like a
/// focus move: reads after it in its own pass get the answer reads before
/// it got, and the next frame routes as if the scope were gone.
///
/// Nested scopes, `root` around `inner`, with one read at `root`'s own
/// position and two inside `inner`, either side of the release. Two
/// rows:
///
/// - **No focus anchor** — focus sits on an unrecorded id, so the grant
///   falls to the layer's outermost, `root`. The read inside `inner`
///   speaks for `inner` and misses. The read at `root` lands.
/// - **Focus inside `inner`** — the path is `[root, inner]` and the grant
///   is `inner`. The read inside `inner` lands and the read at `root`
///   misses.
///
/// On the frame after the release, `inner` is withdrawn in both rows:
/// the grant is `root`, every read speaks for `root`, and all of them
/// land.
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
                reads.at_root = ui.escape_pressed();
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
                        reads.inner_before = ui.escape_pressed();
                        if closes {
                            ui.release_input_scope(WidgetId::from_hash("inner"));
                        }
                        reads.inner_after = ui.escape_pressed();
                    });
            });
        reads
    };
    for focus_inside in [false, true] {
        let mut h = UiHarness::new(glam::UVec2::new(200, 200));
        // `closes` is false on the setup frame: a close outlives its own
        // frame, so closing there would leave `inner` already withdrawn
        // when the pass under test starts.
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

/// Feed an Escape that the keyboard wake-gate will actually deliver,
/// focused on the fixture's `editor` block when it records one.
/// The gate drops an unsubscribed chord when nothing is focused, and
/// `end_frame` evicts focus whose widget was not recorded — so this has
/// to be set immediately before the press, not once up front.
fn press_escape(h: &mut UiHarness) {
    h.set_focus(WidgetId::from_hash("editor"));
    h.key(Key::Escape);
}

/// A node on `layer` declaring an all-taking scope — the shape every
/// overlay reduces to once `modal_layer` became `input_scope`. `body`
/// records inside it, which is what puts a read's `parent` within the
/// scope.
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
    // (label, policy, expect_focus_after_outside_press).
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
        assert_eq!(h.focused_id(), Some(editable_id), "{label}: initial focus");

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
        assert_eq!(h.focused_id(), expected, "{label}: after outside press");
    }
    // Default policy is ClearOnMiss.
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
            fixed_button(WidgetId::from_hash("plain")).show(ui);
        });
    };
    h.frame(build);
    h.click_on(WidgetId::from_hash("editable"));
    assert_eq!(h.focused_id(), Some(WidgetId::from_hash("editable")));

    h.frame(build);
    // Checked: a click that missed `plain` would keep focus too.
    h.click_on(WidgetId::from_hash("plain"));
    assert_eq!(
        h.focused_id(),
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
    assert_eq!(h.focused_id(), Some(WidgetId::from_hash("editable")));

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
fn set_focus_bypasses_policy() {
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let id = WidgetId::from_hash("manual");
    h.set_focus(id);
    assert_eq!(h.focused_id(), Some(id));
    h.clear_focus();
    assert_eq!(h.focused_id(), None);
}

#[test]
fn invisible_or_disabled_focusable_refuses_focus() {
    // Cascade combines `disabled || invisible`; pin both axes so a
    // future split doesn't keep one alive.

    #[derive(Debug)]
    enum Mode {
        Shown,
        Hidden,
        Disabled,
    }
    // `Shown` is the control: the same click on the same spot focuses it.
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
        assert_eq!(h.focused_id(), *expected, "case {label}");
    }
}

#[test]
fn post_record_clears_keys_but_preserves_modifiers() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    state.set_focus(Some(forged_focus()));
    state.feed(InputEvent::ModifiersChanged(Modifiers {
        shift: true,
        ..Modifiers::NONE
    }));
    state.feed(InputEvent::key_down(Key::ArrowLeft));
    let buf_cap_before = state.frame_keyboard_events.capacity();

    state.end_frame(&cascade);

    assert!(state.frame_keyboard_events.is_empty());
    // Capacity-retained: typing across frames stays alloc-free.
    assert_eq!(state.frame_keyboard_events.capacity(), buf_cap_before);
    assert!(state.modifiers.shift);
}
