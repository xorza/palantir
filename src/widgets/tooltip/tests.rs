//! `Tooltip` behavior tests.
//!
//! Multi-frame integration tests drive fake pointer hover at advancing
//! the `Ui` frame-runtime clock to assert visibility, placement, and sizing behavior.

use crate::layout::types::anchor::Anchor;
use crate::ui::frame_report::FrameProcessing;

use crate::input::response::response_state::ResponseState;
use crate::internals::harness::UiHarness;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::tooltip::{Tooltip, TooltipGlobal, TooltipState, global_state_id};
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(400, 300);

#[test]
fn tooltip_near_right_edge_keeps_natural_width() {
    const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
    let reference = visible_tooltip_at(40.0, TEXT);
    let ui = visible_tooltip_at(350.0, TEXT);
    let bubble_id = WidgetId::from_hash("edge-trigger").with("bubble");
    let reference_bubble = reference
        .ui
        .response_for(bubble_id)
        .rect
        .expect("reference tooltip bubble");
    let bubble = ui
        .ui
        .response_for(bubble_id)
        .rect
        .expect("edge tooltip bubble");

    assert_eq!(bubble.size.w, reference_bubble.size.w);
    assert_eq!(bubble.max().x, SURFACE.x as f32);
}

#[test]
fn content_growth_and_shrink_reposition_without_input_or_settling() {
    let short = String::from("tip");
    let long = String::from(
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron",
    );

    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("dynamic-tooltip-trigger");
    let trigger = Rect::new(350.0, 250.0, 40.0, 24.0);
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(trigger),
            pointer_over: true,
            ..ResponseState::default()
        },
    };
    let bubble_id = trigger_id.with("bubble");
    let frame = |h: &mut UiHarness, text: &str| {
        let report = h.frame(|ui| {
            Tooltip::on(&snapshot)
                .label(text)
                .delay(Duration::ZERO)
                .show(ui);
        });
        assert_eq!(
            report.processing,
            FrameProcessing::SingleLayout,
            "tooltip placement must be single-pass"
        );
        h.rect(bubble_id).expect("tooltip bubble arranged")
    };

    let small = frame(&mut h, &short);
    let large = frame(&mut h, &long);
    let shrunk = frame(&mut h, &short);
    let above_edge = trigger.min.y - h.ui.theme().tooltip.gap;

    assert_eq!(small.max().y, above_edge);
    assert_eq!(large.max().y, above_edge);
    // Mono at the tooltip's 13 px: 6.5 px a char, 15.59375 px a line
    // (15.6 snapped to 1/64), inside 6 + 6 by 4 + 4 padding and a 1 px
    // border. "tip" is one 19.5 px line; the long text wraps at
    // 280 − 14 = 266, which holds 40 chars, onto two lines.
    assert_eq!(small.size, Size::new(19.5 + 14.0, 15.59375 + 10.0));
    assert_eq!(large.size, Size::new(260.0 + 14.0, 2.0 * 15.59375 + 10.0));
    assert_eq!(large.max().x, SURFACE.x as f32);
    assert_eq!(
        shrunk, small,
        "shrinking must restore placement immediately"
    );
}

#[test]
fn tooltip_breaks_long_tokens_inside_bubble() {
    let ui = visible_tooltip_at(
        40.0,
        "averylongtooltiptokenwithoutanybreakpointsaverylongtooltiptoken",
    );
    let bubble_id = WidgetId::from_hash("edge-trigger").with("bubble");
    let bubble = ui.rect(bubble_id).expect("tooltip bubble");
    let shaped = ui
        .ui
        .layout(Layer::Tooltip)
        .text_shapes
        .first()
        .expect("tooltip text shaped");
    // Forty 6.5 px chars a line, broken mid-token, inside the 14 px of
    // padding and border.
    assert_eq!(shaped.extent.size, Size::new(260.0, 2.0 * 15.59375));
    assert_eq!(bubble.size.w, 260.0 + 14.0);
    assert!(
        shaped.extent.size.w <= bubble.size.w - ui.ui.theme().tooltip.padding.horizontal_sum(),
        "text width {} must fit inside bubble width {}",
        shaped.extent.size.w,
        bubble.size.w,
    );
}

/// The bubble takes its box from [`Configure`] like any other widget —
/// `Tooltip` used to hand-roll `padding` / `max_size` and offer nothing
/// else, so `margin` here is a setter it simply did not have.
///
/// Identity is the other half: a tooltip has no call site of its own
/// worth keying on, so it derives the bubble id from its trigger — but
/// an explicit `.id(...)` has to win, the same way explicit spacing wins
/// over the theme. Both halves in one test because they are one
/// contract: the builder's surface is `Configure`'s, defaults included.
#[test]
fn configure_reaches_the_bubble_and_explicit_id_beats_the_derived_one() {
    let trigger_id = WidgetId::from_hash("unbounded-tooltip-trigger");
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(Rect::new(40.0, 40.0, 40.0, 24.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Tooltip::on(&snapshot)
            .label("tip")
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .margin(Spacing::all(7.0))
            .max_size(Size::INF)
            .delay(Duration::ZERO)
            .show(ui);
    });

    let derived = trigger_id.with("bubble");
    let bubble = h.node_of(derived).expect("tooltip bubble node");
    assert_eq!(bubble.layer, Layer::Tooltip);
    let tree = h.ui.tree(Layer::Tooltip);
    let index = bubble.node.idx();
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.records.layout()[index].margin, Spacing::all(7.0));
    assert_eq!(tree.bounds(bubble.node).max_size, Size::INF);

    // Same trigger, caller-set id: the derived one must not appear.
    let explicit = WidgetId::from_hash("my-own-bubble");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Tooltip::on(&snapshot)
            .label("tip")
            .id(explicit)
            .delay(Duration::ZERO)
            .show(ui);
    });
    assert_eq!(
        h.node_of(explicit).map(|at| at.layer),
        Some(Layer::Tooltip),
        "an explicit id must reach the recorded bubble",
    );
    assert_eq!(
        h.node_of(derived),
        None,
        "the trigger-derived id must not also be recorded",
    );
}

fn visible_tooltip_at(trigger_x: f32, text: &'static str) -> UiHarness {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("edge-trigger");
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(Rect::new(trigger_x, 40.0, 40.0, 24.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    };
    for why in [
        "measured placement resolves in the layout pass",
        "a measured tooltip stays single-pass",
    ] {
        let report = h.frame(|ui| {
            Tooltip::on(&snapshot)
                .label(text)
                .delay(Duration::ZERO)
                .show(ui);
        });
        assert_eq!(report.processing, FrameProcessing::SingleLayout, "{why}");
    }
    h
}

/// An empty label is nothing to say, so the hover never becomes active:
/// no state row turns visible and the layer stays as empty as it is with
/// no tooltip at all. Same fixture as the visible cases above, which is
/// what makes the empty layer mean something.
#[test]
fn empty_label_records_no_bubble() {
    let empty = visible_tooltip_at(20.0, "");
    let baseline = empty.ui.tree(Layer::Tooltip).records.len();
    assert!(
        !empty
            .ui
            .state::<TooltipState>(WidgetId::from_hash("edge-trigger"))
            .is_some_and(|state| state.visible),
        "an empty tooltip must never become visible",
    );

    let shown = visible_tooltip_at(20.0, "tip");
    assert!(
        shown
            .state::<TooltipState>(WidgetId::from_hash("edge-trigger"))
            .visible,
        "control: the same fixture with text turns its row visible",
    );
    assert_eq!(
        shown.ui.tree(Layer::Tooltip).records.len(),
        baseline + 2,
        "the same fixture with text records the bubble and its label beyond the empty one \
         ({baseline} records)",
    );
}

#[test]
fn tooltip_delay_keeps_subsecond_precision_after_long_uptime() {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("long-uptime-trigger");
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(Rect::new(40.0, 40.0, 40.0, 24.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    };
    let record_at = |h: &mut UiHarness, time: Duration| {
        h.at(time).frame(|ui| {
            Tooltip::on(&snapshot)
                .label("tip")
                .delay(Duration::from_millis(250))
                .show(ui);
        });
    };

    let started_at = Duration::from_secs(1 << 24);
    record_at(&mut h, started_at);
    assert_eq!(
        h.ui.state::<TooltipState>(trigger_id)
            .unwrap()
            .hover_started_at,
        Some(started_at),
    );

    record_at(&mut h, started_at + Duration::from_millis(249));
    assert!(!h.ui.state::<TooltipState>(trigger_id).unwrap().visible);

    record_at(&mut h, started_at + Duration::from_millis(250));
    assert!(h.ui.state::<TooltipState>(trigger_id).unwrap().visible);
}

#[test]
fn tooltip_state_is_swept_with_trigger_while_global_state_persists() {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("transient-trigger");
    let root_id = WidgetId::from_hash("root");
    let record = |ui: &mut Ui| {
        Panel::vstack().id(root_id).show(ui, |ui| {
            let trigger = Button::new().id(trigger_id).label("hi").show(ui).snapshot();
            Tooltip::on(&trigger).label("tip").show(ui);
        });
    };

    h.frame(record);
    assert!(
        h.ui.state::<TooltipState>(trigger_id).is_none(),
        "an idle trigger must not materialise a state row",
    );
    assert!(
        h.ui.state::<TooltipGlobal>(global_state_id()).is_none(),
        "nothing has been visible yet, so the singleton has no row either",
    );

    // Hover lands a frame late — the response reads the previous frame's
    // cascade — so the timer starts on the second frame and the 500 ms
    // theme delay elapses on the third.
    h.move_onto(trigger_id);
    h.frame(record);
    h.advance(Duration::from_millis(600)).frame(record);

    assert!(h.ui.state::<TooltipState>(trigger_id).is_some());
    assert!(
        h.ui.state::<TooltipState>(trigger_id.with("tooltip"))
            .is_none(),
        "per-trigger state must not use an unrecorded synthetic id",
    );
    assert!(
        h.ui.state::<TooltipGlobal>(global_state_id()).is_some(),
        "the intentional global singleton must exist",
    );

    h.advance(Duration::from_millis(16)).frame(|ui| {
        Panel::vstack().id(root_id).show(ui, |_ui| {});
    });

    assert!(h.ui.state::<TooltipState>(trigger_id).is_none());
    assert!(h.ui.state::<TooltipGlobal>(global_state_id()).is_some());
}

/// Drive the timer across N frames with a fixed dt-per-frame, hovering
/// the trigger the entire time. The bubble should be invisible until
/// `time >= delay`, then visible.
#[test]
fn delay_gates_visibility() {
    let mut h = UiHarness::new(SURFACE);

    let mut captured: Option<WidgetId> = None;
    let record_at_secs = |h: &mut UiHarness, secs: f32, captured: &mut Option<WidgetId>| {
        h.at(Duration::from_secs_f32(secs)).frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let r = Button::new()
                        .id(WidgetId::from_hash("trig"))
                        .label("hi")
                        .show(ui)
                        .snapshot();
                    *captured = Some(r.id);
                    Tooltip::on(&r)
                        .label("tip")
                        .delay(Duration::from_millis(300))
                        .show(ui);
                });
        });
    };

    // First frame — pointer not yet over the button. State row exists,
    // but `elapsed == 0` and `visible == false`.
    record_at_secs(&mut h, 0.0, &mut captured);
    let trigger_id = captured.expect("button id");

    h.move_onto(trigger_id);
    record_at_secs(&mut h, 0.05, &mut captured);
    h.move_onto(trigger_id);
    record_at_secs(&mut h, 0.1, &mut captured);
    let early = *h.state::<TooltipState>(trigger_id);
    assert!(
        !early.visible,
        "tooltip must stay hidden before delay elapses"
    );
    assert_eq!(
        early.hover_started_at,
        Some(Duration::from_secs_f32(0.05)),
        "the hover begins on the frame the pointer first reached the trigger",
    );

    // Tick past the delay, hovering the trigger and advancing 0.1 s a
    // frame from 0.2 s. The delay ends at 0.05 + 0.3 = 0.35 s: the 0.3 s
    // frame is 250 ms in and stays hidden, the 0.4 s frame — tick 2 — is
    // 350 ms in and is the first to show.
    let mut t = 0.1_f32;
    let mut first_visible = None;
    for tick in 0..20 {
        t += 0.1;
        h.move_onto(trigger_id);
        record_at_secs(&mut h, t, &mut captured);
        if first_visible.is_none() && h.state::<TooltipState>(trigger_id).visible {
            first_visible = Some(tick);
        }
    }
    assert_eq!(
        first_visible,
        Some(2),
        "the first visible frame is the 0.4 s one"
    );
    assert!(h.state::<TooltipState>(trigger_id).visible);
    assert_eq!(
        h.ui.tree(Layer::Tooltip).records.len(),
        2,
        "the Tooltip layer holds the bubble and its label",
    );

    h.ui.theme_mut().tooltip.warmup = Duration::ZERO;
    t += 0.1;
    h.move_to(Vec2::new(350.0, 250.0));
    record_at_secs(&mut h, t, &mut captured);
    assert!(!h.ui.state::<TooltipState>(trigger_id).unwrap().visible);

    t += 0.1;
    h.move_onto(trigger_id);
    record_at_secs(&mut h, t, &mut captured);
    assert!(
        !h.ui.state::<TooltipState>(trigger_id).unwrap().visible,
        "zero warmup must not bypass the delay on a new hover",
    );
}

/// The bubble records with `Sense::empty()`, so a visible tooltip must
/// never become the hover target: after it appears, moving the pointer
/// off the trigger clears the trigger's hover and hides the bubble.
#[test]
fn hover_clears_after_tooltip_visible() {
    let mut h = UiHarness::new(SURFACE);

    let mut captured: Option<WidgetId> = None;
    let record_at_secs = |h: &mut UiHarness, secs: f32, captured: &mut Option<WidgetId>| {
        h.at(Duration::from_secs_f32(secs)).frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let r = Button::new()
                        .id(WidgetId::from_hash("trig"))
                        .label("hi")
                        .show(ui)
                        .snapshot();
                    *captured = Some(r.id);
                    Tooltip::on(&r)
                        .label("tip")
                        .delay(Duration::from_millis(300))
                        .show(ui);
                });
        });
    };

    record_at_secs(&mut h, 0.0, &mut captured);
    let trigger_id = captured.expect("button id");
    let mut t = 0.0_f32;
    for _ in 0..10 {
        t += 0.1;
        h.move_onto(trigger_id);
        record_at_secs(&mut h, t, &mut captured);
    }
    let state = *h.state::<TooltipState>(trigger_id);
    assert!(
        state.visible,
        "precondition: tooltip visible while hovering"
    );

    // Move the pointer far away from both trigger and bubble.
    let away = Vec2::new(350.0, 250.0);
    h.move_to(away);
    t += 0.1;
    record_at_secs(&mut h, t, &mut captured);

    let state = *h.state::<TooltipState>(trigger_id);
    assert_ne!(
        h.hit_at(away),
        Some(trigger_id),
        "the pointer left the trigger"
    );
    assert!(!state.visible, "tooltip must hide after move-away");
}

/// A tooltip attached to a trigger *inside* a popup body must record
/// into the `Tooltip` layer without tripping the layer-nesting assert:
/// `Tooltip::show` raises `Ui::layer(Tooltip)` while the active scope is
/// already `Popup`. Regression for the panic that forced tooltips out of
/// darkroom's new-node menu.
#[test]
fn tooltip_inside_popup_records_without_panic() {
    use crate::widgets::popup::Popup;
    use crate::widgets::popup::click_outside::ClickOutside;

    let mut h = UiHarness::new(SURFACE);

    // Near top-left so the popup never flips and the trigger stays put.
    let popup_anchor = Vec2::new(40.0, 40.0);
    let mut captured: Option<WidgetId> = None;
    let record_at_secs = |h: &mut UiHarness, secs: f32, captured: &mut Option<WidgetId>| {
        h.at(Duration::from_secs_f32(secs)).frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Popup::new(Anchor::at_point(popup_anchor))
                        .id(WidgetId::from_hash("popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .padding(4.0)
                        .show(ui, |ui, _popup| {
                            let r = Button::new()
                                .id(WidgetId::from_hash("trig"))
                                .label("hi")
                                .show(ui)
                                .snapshot();
                            *captured = Some(r.id);
                            Tooltip::on(&r)
                                .label("tip")
                                .delay(Duration::from_millis(300))
                                .show(ui);
                        });
                });
        });
    };

    // Record once so the trigger rect is available to the next frame.
    record_at_secs(&mut h, 0.0, &mut captured);
    record_at_secs(&mut h, 0.01, &mut captured);
    let trigger_id = captured.expect("button id");
    // Hover the popup-nested trigger and tick past the delay. Each frame
    // re-hovers and advances Ui-time by 0.1 s; hover lag is one frame.
    let mut t = 0.01_f32;
    for _ in 0..20 {
        t += 0.1;
        h.move_onto(trigger_id);
        record_at_secs(&mut h, t, &mut captured);
    }

    let state = *h.state::<TooltipState>(trigger_id);
    assert!(
        state.visible,
        "tooltip on a popup-nested trigger must become visible after the delay (started_at={:?})",
        state.hover_started_at,
    );

    // The bubble records into the Tooltip layer — a root distinct from
    // the Popup layer it was raised inside.
    assert_eq!(
        h.ui.tree(Layer::Tooltip).records.len(),
        2,
        "the Tooltip layer holds the bubble raised inside the popup, and its label",
    );
}

/// A nested layer that ranks at or below the current scope is rejected:
/// with no per-node z-index, `Layer::PAINT_ORDER` is the only ordering,
/// so a `Popup` (1) raised inside a `Modal` (2) body would paint *under*
/// the modal. `push_layer` catches this in every build rather than
/// letting a release one silently misrender.
#[test]
#[should_panic(expected = "must rank above")]
fn layer_below_current_scope_panics() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.layer(Layer::Modal).show(|ui| {
            ui.layer(Layer::Popup).show(|_ui| {});
        });
    });
}

/// A disabled trigger is exactly when the user most wants to be told
/// *why*, so `when_disabled` has to reach one — which needs the
/// pointer to still be observed over a widget that can do nothing with
/// it. The flag stays off by default, so the same fixture without it
/// shows nothing.
#[test]
fn when_disabled_reaches_a_disabled_trigger() {
    let visible_after_hover = |allow: bool| {
        let mut h = UiHarness::new(SURFACE);
        let trigger_id = WidgetId::from_hash("disabled-trigger");
        let record = |h: &mut UiHarness, secs: f32| {
            h.at(Duration::from_secs_f32(secs)).frame_value(|ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        let r = Button::new()
                            .id(trigger_id)
                            .label("save")
                            .disabled(true)
                            .show(ui)
                            .snapshot();
                        Tooltip::on(&r)
                            .label("nothing to save yet")
                            .when_disabled(allow)
                            .delay(Duration::from_millis(300))
                            .show(ui);
                        r.state.disabled
                    })
                    .inner
            })
        };

        assert!(record(&mut h, 0.0), "fixture: the trigger is disabled");
        let mut t = 0.0_f32;
        for _ in 0..10 {
            t += 0.1;
            h.move_onto(trigger_id);
            record(&mut h, t);
        }
        // No row is the off answer: a tooltip that never activates
        // stores nothing. The `true` row is the control that the id is
        // the one a visible tooltip writes.
        h.ui.state::<TooltipState>(trigger_id)
            .is_some_and(|state| state.visible)
    };

    assert!(
        visible_after_hover(true),
        "the flag reaches a disabled trigger"
    );
    assert!(
        !visible_after_hover(false),
        "and off by default it does not"
    );
}

/// The stock tooltip `max_size` (280 wide) is a default, so an authored
/// `min_size` above it raises the bound instead of panicking.
#[test]
fn an_authored_min_above_the_themed_max_width_wins() {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("wide-tip-trigger");
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(Rect::new(20.0, 40.0, 40.0, 24.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    };
    h.prime(2, |ui| {
        Tooltip::on(&snapshot)
            .label("wide")
            .delay(Duration::ZERO)
            .min_size((300.0, 0.0))
            .show(ui);
    });
    assert!(h.state::<TooltipState>(trigger_id).visible);
}
