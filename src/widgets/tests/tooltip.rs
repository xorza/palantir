//! `Tooltip` behavior tests.
//!
//! Two pure-fn tests pin `place_anchor` (default-below / flip-above /
//! horizontal clamp). One multi-frame integration test drives a fake
//! pointer hover at advancing `Ui::time` to assert the delay actually
//! gates `TooltipState.visible`.

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::InputEvent;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::tooltip::{Tooltip, TooltipState, place_anchor};
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(400, 300);

#[test]
fn place_anchor_below_when_room() {
    let viewport = Rect {
        min: Vec2::ZERO,
        size: Size::new(400.0, 300.0),
    };
    let trigger = Rect {
        min: Vec2::new(50.0, 50.0),
        size: Size::new(80.0, 24.0),
    };
    let bubble = Size::new(120.0, 32.0);
    let placed = place_anchor(trigger, bubble, viewport, 6.0);
    assert!(!placed.flipped_above);
    assert!((placed.anchor.x - 50.0).abs() < 1e-4);
    assert!((placed.anchor.y - (50.0 + 24.0 + 6.0)).abs() < 1e-4);
}

#[test]
fn place_anchor_flips_above_at_bottom_edge() {
    let viewport = Rect {
        min: Vec2::ZERO,
        size: Size::new(400.0, 300.0),
    };
    // Trigger sitting at the very bottom of the viewport.
    let trigger = Rect {
        min: Vec2::new(50.0, 270.0),
        size: Size::new(80.0, 24.0),
    };
    let bubble = Size::new(120.0, 32.0);
    let placed = place_anchor(trigger, bubble, viewport, 6.0);
    assert!(placed.flipped_above);
    assert!((placed.anchor.y - (270.0 - 6.0 - 32.0)).abs() < 1e-4);
}

#[test]
fn place_anchor_clamps_horizontally() {
    let viewport = Rect {
        min: Vec2::ZERO,
        size: Size::new(400.0, 300.0),
    };
    // Trigger near the right edge; bubble is wider than the trigger and
    // would overflow the viewport without clamping.
    let trigger = Rect {
        min: Vec2::new(350.0, 50.0),
        size: Size::new(40.0, 24.0),
    };
    let bubble = Size::new(120.0, 32.0);
    let placed = place_anchor(trigger, bubble, viewport, 6.0);
    assert!((placed.anchor.x - (400.0 - 120.0)).abs() < 1e-4);
}

/// Drive the timer across N frames with a fixed dt-per-frame, hovering
/// the trigger the entire time. The bubble should be invisible until
/// `time >= delay`, then visible.
#[test]
fn delay_gates_visibility() {
    let mut ui = Ui::new();
    let display = Display::from_physical(SURFACE, 1.0);

    let mut captured: Option<WidgetId> = None;
    let frame_at = |ui: &mut Ui, secs: f32, captured: &mut Option<WidgetId>| {
        ui.frame(display, Duration::from_secs_f32(secs), |ui| {
            Panel::vstack()
                .id_salt("root")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let r = Button::new().id_salt("trig").label("hi").show(ui);
                    *captured = Some(r.widget_id());
                    Tooltip::for_(&r).text("tip").delay(0.3).show(ui);
                });
        });
    };

    // First frame — pointer not yet over the button. State row exists,
    // but `elapsed == 0` and `visible == false`.
    frame_at(&mut ui, 0.0, &mut captured);
    let trigger_id = captured.expect("button id");
    let state_id = trigger_id.with("tooltip");

    // Move pointer to the center of the button's last-frame rect.
    let trigger_rect = ui.response_for(trigger_id).rect.expect("button rect");
    let trigger_pos =
        trigger_rect.min + Vec2::new(trigger_rect.size.w * 0.5, trigger_rect.size.h * 0.5);

    ui.on_input(InputEvent::PointerMoved(trigger_pos));
    frame_at(&mut ui, 0.05, &mut captured);
    ui.on_input(InputEvent::PointerMoved(trigger_pos));
    frame_at(&mut ui, 0.1, &mut captured);
    let early = ui
        .try_state::<TooltipState>(state_id)
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
        ui.on_input(InputEvent::PointerMoved(trigger_pos));
        frame_at(&mut ui, t, &mut captured);
    }

    let late = ui
        .try_state::<TooltipState>(state_id)
        .copied()
        .unwrap_or_default();
    assert!(
        late.visible,
        "tooltip must become visible after delay (started_at={:?})",
        late.hover_started_at
    );

    // The tooltip subtree should have been recorded into the Tooltip
    // layer (non-empty NodeRecords beyond the root).
    let tooltip_tree = &ui.forest.trees[Layer::Tooltip as usize];
    assert!(
        tooltip_tree.records.len() > 1,
        "Tooltip layer must contain at least one recorded node",
    );
}
