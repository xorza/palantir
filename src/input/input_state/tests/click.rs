use crate::Ui;
use crate::input::capture::DOUBLE_CLICK_WINDOW;
use crate::input::input_state::tests::{BUTTON_SURFACE, build_button, fixed_button};
use crate::input::sense::Sense;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::{button::Button, panel::Panel};
use glam::{UVec2, Vec2};
use std::time::Duration;

/// A gap inside [`DOUBLE_CLICK_WINDOW`], explicit because the harness clock
/// stands still until moved.
const IN_WINDOW: Duration = Duration::from_millis(100);
const _: () = assert!(IN_WINDOW.as_millis() < DOUBLE_CLICK_WINDOW.as_millis());

#[test]
fn input_state_press_release_emits_click() {
    // Frame 1 lays out the button; frame 2 reads .left.clicked() after a press +
    // release inside its rect; frame 3 confirms the click is one-shot.
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = build_button(WidgetId::from_hash("target"));
    h.frame(build);
    h.click_at(Vec2::new(50.0, 20.0));

    let clicked = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                fixed_button(WidgetId::from_hash("target"))
                    .label("hi")
                    .show(ui)
                    .left
                    .clicked()
            })
            .inner
    };
    assert!(
        h.frame_value(clicked),
        "press+release inside button rect should click"
    );
    assert!(!h.frame_value(clicked), "click is one-shot");
}

#[test]
fn stack_sense_routing() {
    let cases: &[(&str, Sense, Vec2, bool, bool, bool)] = &[
        (
            "sense_none_passes_through",
            Sense::NONE,
            Vec2::new(5.0, 5.0),
            false,
            false,
            false,
        ),
        (
            "sense_click_captures_background",
            Sense::CLICK,
            Vec2::new(5.0, 5.0),
            true,
            true,
            false,
        ),
        (
            "sense_hover_reports_hover_only",
            Sense::HOVER,
            Vec2::new(5.0, 5.0),
            false,
            true,
            false,
        ),
    ];
    for (label, sense, click_pos, expect_stack_click, expect_stack_hover, expect_child_click) in
        cases
    {
        let surface = UVec2::new(200, 100);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("stack"))
                .padding(20.0)
                .sense(*sense)
                .show(ui, |ui| {
                    Button::new()
                        .id(WidgetId::from_hash("inside"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        };
        h.frame(build);
        h.click_at(*click_pos);

        let [stack_clicked, stack_hovered, child_clicked] = h.frame_value(|ui| {
            let r = Panel::hstack()
                .id(WidgetId::from_hash("stack"))
                .padding(20.0)
                .sense(*sense)
                .show(ui, |ui| {
                    Button::new()
                        .id(WidgetId::from_hash("inside"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .left
                        .clicked()
                });
            [r.response.left.clicked(), r.response.hovered(), r.inner]
        });
        assert_eq!(
            stack_clicked, *expect_stack_click,
            "case {label}: stack clicked"
        );
        assert_eq!(
            stack_hovered, *expect_stack_hover,
            "case {label}: stack hovered"
        );
        assert_eq!(
            child_clicked, *expect_child_click,
            "case {label}: child clicked"
        );
    }
}

/// A disabled widget covers what it is painted over: it keeps its declared
/// sense, so the press still routes to it, but it answers nothing; the widget
/// beneath neither clicks nor takes focus.
///
/// The `Sense::NONE` case is the control: an inert cover is a hole, so only
/// the disabling stops the press in the other case.
#[test]
fn a_disabled_cover_absorbs_the_press_it_is_painted_over() {
    use crate::widgets::block::Block;

    let under = WidgetId::from_hash("under");
    let cover = WidgetId::from_hash("cover");
    for (label, sense, disabled, expect_click, expect_focus) in [
        ("inert cover", Sense::NONE, false, true, Some(under)),
        ("disabled cover", Sense::CLICK, true, false, None),
    ] {
        let mut h = UiHarness::new(UVec2::splat(100));
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .auto_id()
                .size(Sizing::fixed(100.0))
                .show(ui, |ui| {
                    let clicked = Block::new()
                        .id(under)
                        .size(Sizing::FILL)
                        .sense(Sense::CLICK)
                        .focusable(true)
                        .show(ui)
                        .left
                        .clicked();
                    Block::new()
                        .id(cover)
                        .size(Sizing::FILL)
                        .sense(sense)
                        .disabled(disabled)
                        .show(ui);
                    clicked
                })
                .inner
        };
        h.frame(|ui| {
            build(ui);
        });
        h.click_at(Vec2::splat(50.0));
        let clicked = h.frame_value(build);

        assert_eq!(clicked, expect_click, "{label}: the widget underneath");
        assert_eq!(h.focus(), expect_focus, "{label}: focus");
    }
}

#[test]
fn input_state_release_outside_does_not_click() {
    let surface = UVec2::new(400, 80);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(WidgetId::from_hash("target")).show(ui);
        });
    });
    let target = WidgetId::from_hash("target");
    let (inside, outside) = (Vec2::new(50.0, 20.0), Vec2::new(300.0, 20.0));
    assert_eq!(
        h.hit_at(inside),
        Some(target),
        "the press lands on the button"
    );
    assert_ne!(h.hit_at(outside), Some(target), "the release is off it");
    h.press_at(inside);
    h.drag_to(outside);
    h.release();

    let got_click = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                fixed_button(WidgetId::from_hash("target"))
                    .show(ui)
                    .left
                    .clicked()
            })
            .inner
    });
    assert!(
        !got_click,
        "release outside the original widget cancels click"
    );
}

#[test]
fn click_on_overflow_outside_clipped_parent_is_suppressed() {
    let surface = UVec2::new(400, 400);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("clipper"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                    .clip_rect()
                    .show(ui, |ui| {
                        Button::new()
                            .id(WidgetId::from_hash("inner"))
                            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                            .show(ui)
                            .left
                            .clicked()
                    })
                    .inner
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });
    // The control: the same button clicks where the clip shows it.
    let inner = WidgetId::from_hash("inner");
    let (shown, overflow) = (Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0));
    assert_eq!(h.hit_at(shown), Some(inner));
    h.click_at(shown);
    assert!(h.frame_value(build), "a click inside the clip registers");

    assert_ne!(
        h.hit_at(overflow),
        Some(inner),
        "the clip hides the overflow"
    );
    h.click_at(overflow);
    assert!(
        !h.frame_value(build),
        "click on overflow outside clip should not register"
    );
}

#[test]
fn transformed_panels_route_clicks_by_composed_world_rect() {
    use crate::primitives::geometry::translate_scale::TranslateScale;
    let cases = [
        (
            "scale_2x_inside",
            TranslateScale::IDENTITY,
            TranslateScale::from_scale(2.0),
            Vec2::new(75.0, 75.0),
            true,
        ),
        (
            "scale_0.5x_outside_world",
            TranslateScale::IDENTITY,
            TranslateScale::from_scale(0.5),
            Vec2::new(40.0, 40.0),
            false,
        ),
        (
            "nested_composition_inside",
            TranslateScale::new(Vec2::new(10.0, 20.0), 2.0),
            TranslateScale::new(Vec2::new(5.0, 7.0), 3.0),
            Vec2::new(250.0, 250.0),
            true,
        ),
        (
            "nested_composition_before_composed_min",
            TranslateScale::new(Vec2::new(10.0, 20.0), 2.0),
            TranslateScale::new(Vec2::new(5.0, 7.0), 3.0),
            Vec2::new(15.0, 30.0),
            false,
        ),
    ];
    for (label, parent_transform, child_transform, click_pos, expect) in cases {
        let surface = UVec2::new(400, 400);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("zoomer"))
                        .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                        .transform(parent_transform)
                        .show(ui, |ui| {
                            Panel::zstack()
                                .id(WidgetId::from_hash("inner_transform"))
                                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                                .transform(child_transform)
                                .show(ui, |ui| {
                                    Button::new()
                                        .id(WidgetId::from_hash("inner"))
                                        .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                                        .show(ui)
                                        .left
                                        .clicked()
                                })
                                .inner
                        })
                        .inner
                })
                .inner
        };
        h.frame(|ui| {
            build(ui);
        });
        h.click_at(click_pos);
        let clicked = h.frame_value(build);
        assert_eq!(clicked, expect, "case {label}");
    }
}

#[test]
fn secondary_click_press_release_emits_secondary_clicked() {
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let r = fixed_button(WidgetId::from_hash("rc_target"))
                    .label("rc")
                    .show(ui);
                [r.left.clicked(), r.right.clicked()]
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });
    h.right_click_at(Vec2::new(50.0, 20.0));

    assert_eq!(
        h.frame_value(build),
        [false, true],
        "a right press+release clicks right, and only right"
    );
    assert_eq!(h.frame_value(build), [false, false], "a click is one-shot");
}

#[test]
fn two_left_clicks_within_window_emit_double_clicked() {
    // Two clicks within DOUBLE_CLICK_WINDOW set `double_clicked` on the second;
    // the first alone must not.
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let r = fixed_button(WidgetId::from_hash("dc_target"))
                    .label("dc")
                    .show(ui);
                [r.left.clicked(), r.left.double_clicked()]
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });

    h.click_at(Vec2::new(50.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "first click should fire `clicked`");
    assert!(!double, "first click must not fire `double_clicked`");

    h.advance(IN_WINDOW);
    h.click_at(Vec2::new(50.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "second click should still fire `clicked`");
    assert!(double, "second click should fire `double_clicked`");

    let [_, still] = h.frame_value(build);
    assert!(!still, "double_clicked is one-shot");

    // A third click must NOT re-fire: the timer reset on the previous fire.
    h.advance(IN_WINDOW);
    h.click_at(Vec2::new(50.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "third click should fire `clicked`");
    assert!(!double, "third click must not chain another double");
}

/// A double click after an idle gap is still a double click. A press carries
/// its arrival time, and an event-driven host runs no frame while idle; stamped
/// with the frame clock, the first press would carry the last frame's time and
/// measure the idle instead of the 100 ms between presses.
#[test]
fn a_double_click_survives_the_idle_before_it() {
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let r = fixed_button(WidgetId::from_hash("idle_target"))
                    .label("dc")
                    .show(ui);
                [r.left.clicked(), r.left.double_clicked()]
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });

    h.advance(Duration::from_secs(10));
    h.click_at(Vec2::new(50.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "the waking click is a click");
    assert!(!double, "and the first of a pair is not a double");

    h.advance(IN_WINDOW);
    h.click_at(Vec2::new(50.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "the second click is a click");
    assert!(
        double,
        "two presses 100 ms apart are a double click, whatever the app \
         was doing for the ten seconds before them",
    );
}

#[test]
fn two_clicks_outside_radius_do_not_double_click() {
    // Within the window, but the second press is more than `DOUBLE_CLICK_RADIUS`
    // away: a slow drift is two clicks.
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let r = Button::new()
                    .id(WidgetId::from_hash("dc_radius_target"))
                    .label("dc")
                    .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                    .show(ui);
                [r.left.clicked(), r.left.double_clicked()]
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });

    h.click_at(Vec2::new(20.0, 20.0));
    assert_eq!(
        h.frame_value(build),
        [true, false],
        "control: the first click"
    );

    // Second click inside the window but ~20px away: must NOT double.
    h.advance(IN_WINDOW);
    h.click_at(Vec2::new(40.0, 20.0));
    let [single, double] = h.frame_value(build);
    assert!(single, "control: the second press lands on the button");
    assert!(
        !double,
        "clicks more than DOUBLE_CLICK_RADIUS apart must not double-click"
    );
}

#[test]
fn click_on_different_widget_resets_double_click() {
    // Different widgets within the window must NOT double; the gesture is per-id.
    let surface = UVec2::new(300, 80);
    let mut h = UiHarness::new(surface);
    // Each button's click count this frame.
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                ["dc_a", "dc_b"].map(|id| {
                    fixed_button(WidgetId::from_hash(id))
                        .label(id)
                        .show(ui)
                        .left
                        .click_count()
                })
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });

    h.click_at(Vec2::new(50.0, 20.0)); // hits A
    assert_eq!(h.frame_value(build), [1, 0], "control: A takes one click");
    h.advance(IN_WINDOW);
    h.click_at(Vec2::new(150.0, 20.0)); // hits B
    assert_eq!(
        h.frame_value(build),
        [0, 1],
        "B's first click is a single, not the second of A's pair",
    );
}

#[test]
fn left_and_right_click_are_independent() {
    use crate::input::pointer::PointerButton;
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let r = fixed_button(WidgetId::from_hash("indep"))
                    .label("x")
                    .show(ui);
                [r.left.clicked(), r.right.clicked()]
            })
            .inner
    };
    h.frame(|ui| {
        build(ui);
    });

    // Left-press, then a right press+release while left is held: latch separately.
    h.press_at(Vec2::new(50.0, 20.0));
    h.press_button(PointerButton::Right);
    h.release_button(PointerButton::Right);
    h.release();

    let [lc, rc] = h.frame_value(build);
    assert!(lc, "left click should still fire");
    assert!(rc, "right click should still fire alongside left");
}

/// An action absorbed by a frame that never records (inert click under
/// `OnDelta`, then a PaintOnly wake) must not stay latched, or the next record
/// pass sees `take_action_flag() == true` with empty queues and runs a spurious
/// second layout pass.
#[test]
fn drain_per_frame_queues_clears_action_latch() {
    use crate::input::input_state::InputState;
    let mut input = InputState {
        frame_had_action: true,
        ..Default::default()
    };
    input.drain_per_frame_queues();
    assert!(!input.take_action_flag());
}

/// `press_count` numbers the multi-press run on the press edge (unlike
/// `clicked`, on release): same-target presses within the window and radius
/// chain 1, 2, 3; one past the radius restarts at 1. Only the processing frame
/// carries it; others read 0.
#[test]
fn press_started_counts_multi_press_runs() {
    fn probe(h: &mut UiHarness) -> u8 {
        let id = WidgetId::from_hash("target");
        h.frame_value(|ui| {
            build_button(id)(ui);
            let r = ui.response_for(id);
            r.left.press_count()
        })
    }

    let mut h = UiHarness::new(BUTTON_SURFACE);
    probe(&mut h); // settle layout

    h.press_on(WidgetId::from_hash("target"));
    assert_eq!(probe(&mut h), 1, "first press starts a run");
    h.release();
    assert_eq!(probe(&mut h), 0, "the count clears off the press frame");

    h.advance(IN_WINDOW);
    h.press();
    assert_eq!(probe(&mut h), 2, "same-spot follow-up chains");
    h.release();
    probe(&mut h);

    h.advance(IN_WINDOW);
    h.press();
    assert_eq!(probe(&mut h), 3, "third press keeps counting");
    h.release();
    probe(&mut h);

    // Past DOUBLE_CLICK_RADIUS (5 px), inside the window: the run restarts.
    h.advance(IN_WINDOW);
    h.press_at(Vec2::new(80.0, 20.0));
    assert_eq!(probe(&mut h), 1, "far press restarts the run");
    h.release();
}

/// **The collation and the poll never disagree about what happened.**
///
/// [`Ui::pointer_actions`] and [`Ui::response_for`] read the same capture
/// state from opposite ends, so every edge below is checked against the
/// `Response` the same frame returns.
///
/// Read per pass: a frame that settles input records twice and the second pass
/// gets none, so pass B is `false` for the poll and empty for the collation.
#[test]
fn pointer_actions_report_the_edges_the_response_reports() {
    use crate::input::interaction::button_phase::ButtonPhase;
    use crate::input::interaction::pointer_action::PointerAction;
    use crate::input::interaction::pointer_edge::PointerEdge;
    use crate::input::pointer::PointerButton;

    let id = WidgetId::from_hash("collated");
    let mut h = UiHarness::new(BUTTON_SURFACE);
    let build = build_button(id);
    h.frame(build);

    let at = |edge| PointerAction {
        id,
        button: PointerButton::Left,
        edge,
    };

    // The press frame. The capture latches on press and is destroyed by release,
    // so no frame carries both edges.
    h.press_at(Vec2::new(50.0, 20.0));
    let pressed = h.frame_passes(|ui| {
        let edges = ui.pointer_actions().collect::<Vec<_>>();
        let down = matches!(ui.response_for(id).left.phase, ButtonPhase::Down { .. });
        build(ui);
        (edges, down)
    });
    assert_eq!(
        *pressed.a(),
        (vec![at(PointerEdge::Pressed { count: 1 })], true),
        "the response and the collation agree on the press",
    );
    assert_eq!(pressed.len(), 1, "the press settles nothing, so no pass B");

    // The release frame.
    h.release();
    let released = h.frame_passes(|ui| {
        let edges = ui.pointer_actions().collect::<Vec<_>>();
        let clicked = ui.response_for(id).left.clicked();
        build(ui);
        (edges, clicked)
    });
    assert_eq!(
        *released.a(),
        (vec![at(PointerEdge::Clicked { count: 1 })], true),
        "the response and the collation agree on the click",
    );
    assert_eq!(
        released.b(),
        Some(&(vec![], false)),
        "and on pass B's silence"
    );

    // A quiet frame collates nothing — these are edges, not levels.
    let quiet = h.frame_value(|ui| {
        let edges = ui.pointer_actions().collect::<Vec<_>>();
        build(ui);
        edges
    });
    assert!(
        quiet.is_empty(),
        "an edge outlived the frame it happened on"
    );
}
