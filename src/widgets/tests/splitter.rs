//! Splitter divider drag: pointer→ratio mapping through last frame's
//! arranged extent, clamping at the stops, and the resulting pane
//! re-layout.

use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::splitter::Splitter;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(500, 300);

fn split_id() -> WidgetId {
    WidgetId::from_hash("split")
}

/// One frame: a 406×100 horizontal splitter at the surface origin.
/// Default theme thickness is 6, so the free span is 400 — divider
/// center at x = ratio · 400 + 3.
fn frame_with(ui: &mut Ui, ratio: &mut f32) {
    ui.run_at_acked(SURFACE, |ui| {
        Splitter::horizontal(ratio)
            .id(split_id())
            .size((Sizing::Fixed(406.0), Sizing::Fixed(100.0)))
            .min_pane(50.0)
            .show(ui, |_| {}, |_| {});
    });
}

#[test]
fn divider_drag_maps_pointer_to_ratio_and_relayouts() {
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    frame_with(&mut ui, &mut ratio);

    // ratio 0.5 → first pane [0, 200), divider [200, 206). Press its
    // center and drag 100 px right: pointer 303 → first = 300 → 0.75.
    ui.press_at(Vec2::new(203.0, 50.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(303.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.75).abs() < 1e-6,
        "pointer 303 over span 400 → 0.75, got {ratio}"
    );

    // The next frame arranges the panes from the new ratio: first pane
    // spans 0.75 · 400 = 300 px.
    frame_with(&mut ui, &mut ratio);
    let first = ui.node_for_widget_id(split_id().with("first"));
    let rect = ui.layout[Layer::Main].rect[first.idx()];
    assert!(
        (rect.size.w - 300.0).abs() < 0.5,
        "first pane arranged to 300 px, got {}",
        rect.size.w
    );

    // Dragging far past the end clamps at the min_pane stop:
    // second pane floors at 50 → first = 350 → 0.875.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(999.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.875).abs() < 1e-6,
        "min_pane(50) stops at 350/400, got {ratio}"
    );

    // Release ends the gesture; further pointer motion leaves the
    // ratio alone.
    ui.release_left();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.875).abs() < 1e-6,
        "ratio holds after release, got {ratio}"
    );
}
