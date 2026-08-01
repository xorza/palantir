//! `Tooltip` behavior tests.
//!
//! Multi-frame integration tests drive fake pointer hover at advancing
//! the `Ui` frame-runtime clock to assert visibility, placement, and sizing behavior.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::tooltip::{GLOBAL_STATE_ID, Tooltip, TooltipGlobal, TooltipState};
use crate::widgets::{ResponseSnapshot, ResponseState};
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(400, 300);

#[test]
fn tooltip_near_right_edge_keeps_natural_width() {
    const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
    let reference = visible_tooltip_at(40.0, TEXT);
    let ui = visible_tooltip_at(350.0, TEXT);
    let bubble_id = WidgetId::from_hash("edge-trigger").with("tooltip.bubble");
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
            hovered: true,
            ..ResponseState::default()
        },
    };
    let bubble_id = trigger_id.with("tooltip.bubble");
    let frame = |h: &mut UiHarness, text: &str| {
        let mut passes = 0;
        h.frame(|ui| {
            passes += 1;
            Tooltip::on(&snapshot)
                .text(text)
                .delay(Duration::ZERO)
                .show(ui);
        });
        assert_eq!(passes, 1, "tooltip placement must be single-pass");
        h.rect(bubble_id).expect("tooltip bubble arranged")
    };

    let small = frame(&mut h, &short);
    let large = frame(&mut h, &long);
    let shrunk = frame(&mut h, &short);
    let above_edge = trigger.min.y - h.ui.theme.tooltip.gap;

    assert_eq!(small.max().y, above_edge);
    assert_eq!(large.max().y, above_edge);
    assert!(
        large.size.w > small.size.w || large.size.h > small.size.h,
        "long content must change the measured bubble size",
    );
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
    let bubble_id = WidgetId::from_hash("edge-trigger").with("tooltip.bubble");
    let bubble = ui.ui.response_for(bubble_id).rect.expect("tooltip bubble");
    let shaped = ui.ui.layout[Layer::Tooltip]
        .text_shapes
        .first()
        .expect("tooltip text shaped");
    assert!(
        shaped.measured.w <= bubble.size.w - ui.ui.theme.tooltip.padding.horiz(),
        "text width {} must fit inside bubble width {}",
        shaped.measured.w,
        bubble.size.w,
    );
}

/// Record index of the bubble node carrying `id`, or `None` when no
/// node in the tooltip layer has it.
fn bubble_node(h: &UiHarness, id: WidgetId) -> Option<usize> {
    h.ui.forest.trees[Layer::Tooltip]
        .records
        .widget_id()
        .iter()
        .position(|recorded| *recorded == id)
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
            hovered: true,
            ..ResponseState::default()
        },
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Tooltip::on(&snapshot)
            .text("tip")
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .margin(Spacing::all(7.0))
            .max_size(Size::INF)
            .delay(Duration::ZERO)
            .show(ui);
    });

    let derived = trigger_id.with("tooltip.bubble");
    let index = bubble_node(&h, derived).expect("tooltip bubble node");
    let tree = &h.ui.forest.trees[Layer::Tooltip];
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.records.layout()[index].margin, Spacing::all(7.0));
    assert_eq!(tree.bounds(NodeId(index as u32)).max_size, Size::INF);

    // Same trigger, caller-set id: the derived one must not appear.
    let explicit = WidgetId::from_hash("my-own-bubble");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Tooltip::on(&snapshot)
            .text("tip")
            .id(explicit)
            .delay(Duration::ZERO)
            .show(ui);
    });
    assert!(
        bubble_node(&h, explicit).is_some(),
        "an explicit id must reach the recorded bubble",
    );
    assert!(
        bubble_node(&h, derived).is_none(),
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
            hovered: true,
            ..ResponseState::default()
        },
    };
    let mut passes = 0;
    h.frame(|ui| {
        passes += 1;
        Tooltip::on(&snapshot)
            .text(text)
            .delay(Duration::ZERO)
            .show(ui);
    });

    assert_eq!(passes, 1, "measured placement resolves in the layout pass");
    passes = 0;
    h.frame(|ui| {
        passes += 1;
        Tooltip::on(&snapshot)
            .text(text)
            .delay(Duration::ZERO)
            .show(ui);
    });
    assert_eq!(passes, 1, "a measured tooltip stays single-pass");
    h
}

#[test]
fn tooltip_delay_keeps_subsecond_precision_after_long_uptime() {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("long-uptime-trigger");
    let snapshot = ResponseSnapshot {
        id: trigger_id,
        state: ResponseState {
            rect: Some(Rect::new(40.0, 40.0, 40.0, 24.0)),
            hovered: true,
            ..ResponseState::default()
        },
    };
    let record_at = |h: &mut UiHarness, time: Duration| {
        h.at(time).frame(|ui| {
            Tooltip::on(&snapshot)
                .text("tip")
                .delay(Duration::from_millis(250))
                .show(ui);
        });
    };

    let started_at = Duration::from_secs(1 << 24);
    record_at(&mut h, started_at);
    assert_eq!(
        h.ui.try_state::<TooltipState>(trigger_id)
            .unwrap()
            .hover_started_at,
        Some(started_at),
    );

    record_at(&mut h, started_at + Duration::from_millis(249));
    assert!(!h.ui.try_state::<TooltipState>(trigger_id).unwrap().visible);

    record_at(&mut h, started_at + Duration::from_millis(250));
    assert!(h.ui.try_state::<TooltipState>(trigger_id).unwrap().visible);
}

#[test]
fn tooltip_state_is_swept_with_trigger_while_global_state_persists() {
    let mut h = UiHarness::new(SURFACE);
    let trigger_id = WidgetId::from_hash("transient-trigger");
    let root_id = WidgetId::from_hash("root");

    h.frame(|ui| {
        Panel::vstack().id(root_id).show(ui, |ui| {
            let trigger = Button::new().id(trigger_id).label("hi").show(ui).snapshot();
            Tooltip::on(&trigger).text("tip").show(ui);
        });
    });

    assert!(h.ui.try_state::<TooltipState>(trigger_id).is_some());
    assert!(
        h.ui.try_state::<TooltipState>(trigger_id.with("tooltip"))
            .is_none(),
        "per-trigger state must not use an unrecorded synthetic id",
    );
    assert!(
        h.ui.try_state::<TooltipGlobal>(*GLOBAL_STATE_ID).is_some(),
        "the intentional global singleton must exist",
    );

    h.at(Duration::from_millis(16)).frame(|ui| {
        Panel::vstack().id(root_id).show(ui, |_ui| {});
    });

    assert!(h.ui.try_state::<TooltipState>(trigger_id).is_none());
    assert!(h.ui.try_state::<TooltipGlobal>(*GLOBAL_STATE_ID).is_some());
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
                        .text("tip")
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
    let early =
        h.ui.try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
    assert!(
        !early.visible,
        "tooltip must stay hidden before delay elapses (started_at={:?})",
        early.hover_started_at
    );

    // Tick well past the delay. The cascade lag is one frame, so we
    // pad with extra ticks; each one hovers the trigger and advances
    // time by 0.1 s.
    let mut t = 0.1_f32;
    for _ in 0..20 {
        t += 0.1;
        h.move_onto(trigger_id);
        record_at_secs(&mut h, t, &mut captured);
    }

    let late =
        h.ui.try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
    assert!(
        late.visible,
        "tooltip must become visible after delay (started_at={:?})",
        late.hover_started_at
    );
    let tooltip_tree = &h.ui.forest.trees[Layer::Tooltip];
    assert!(
        tooltip_tree.records.len() > 1,
        "Tooltip layer must contain at least one recorded node",
    );

    h.ui.theme.tooltip.warmup = Duration::ZERO;
    t += 0.1;
    h.move_to(Vec2::new(350.0, 250.0));
    record_at_secs(&mut h, t, &mut captured);
    assert!(!h.ui.try_state::<TooltipState>(trigger_id).unwrap().visible);

    t += 0.1;
    h.move_onto(trigger_id);
    record_at_secs(&mut h, t, &mut captured);
    assert!(
        !h.ui.try_state::<TooltipState>(trigger_id).unwrap().visible,
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
                        .text("tip")
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
    let state =
        h.ui.try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
    assert!(
        state.visible,
        "precondition: tooltip visible while hovering"
    );

    // Move the pointer far away from both trigger and bubble.
    let away = Vec2::new(350.0, 250.0);
    h.move_to(away);
    t += 0.1;
    record_at_secs(&mut h, t, &mut captured);

    let hovered = h.ui.response_for(trigger_id).hovered;
    let state =
        h.ui.try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
    assert!(!hovered, "trigger must not be hovered after move-away");
    assert!(!state.visible, "tooltip must hide after move-away");
}

/// A tooltip attached to a trigger *inside* a popup body must record
/// into the `Tooltip` layer without tripping the layer-nesting assert:
/// `Tooltip::show` raises `Ui::layer(Tooltip)` while the active scope is
/// already `Popup`. Regression for the panic that forced tooltips out of
/// darkroom's new-node menu.
#[test]
fn tooltip_inside_popup_records_without_panic() {
    use crate::widgets::popup::{ClickOutside, Popup};

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
                    Popup::anchored_to(popup_anchor)
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
                                .text("tip")
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

    let state =
        h.ui.try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
    assert!(
        state.visible,
        "tooltip on a popup-nested trigger must become visible after the delay (started_at={:?})",
        state.hover_started_at,
    );

    // The bubble records into the Tooltip layer — a root distinct from
    // the Popup layer it was raised inside.
    let tooltip_tree = &h.ui.forest.trees[Layer::Tooltip];
    assert!(
        tooltip_tree.records.len() > 1,
        "Tooltip layer must contain the bubble recorded from inside the popup",
    );
}

/// A nested layer that ranks at or below the current scope is rejected:
/// with no per-node z-index, `Layer::PAINT_ORDER` is the only ordering,
/// so a `Popup` (1) raised inside a `Modal` (2) body would paint *under*
/// the modal. `push_layer` must catch this rather than silently misrender.
#[test]
#[should_panic(expected = "must rank above")]
fn layer_below_current_scope_panics() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.layer(Layer::Modal, Vec2::ZERO, None, |ui| {
            ui.layer(Layer::Popup, Vec2::ZERO, None, |_ui| {});
        });
    });
}
