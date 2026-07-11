//! Splitter divider drag: pointer→ratio mapping through last frame's
//! arranged extent, clamping at the stops, the resulting pane
//! re-layout, and the resize-cursor request.

use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::splitter::Splitter;
use crate::window::CursorIcon;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(500, 300);

fn split_id() -> WidgetId {
    WidgetId::from_hash("split")
}

/// One frame: a 401×100 horizontal splitter at the surface origin.
/// Default theme reserves the 1 px rule, so the free span is 400 —
/// seam center at x = ratio · 400 + 0.5, with the 6 px grab bar
/// straddling it. The bar is placed from *last* frame's extent, so
/// tests run two warm-up frames before interacting.
fn frame_with(ui: &mut Ui, ratio: &mut f32) {
    ui.run_at_acked(SURFACE, |ui| {
        Splitter::horizontal(ratio)
            .id(split_id())
            .size((Sizing::Fixed(401.0), Sizing::Fixed(100.0)))
            .min_pane(50.0)
            .show(ui, |_, _| {});
    });
}

#[test]
fn divider_drag_maps_pointer_to_ratio_and_relayouts() {
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    frame_with(&mut ui, &mut ratio);
    frame_with(&mut ui, &mut ratio);

    // ratio 0.5 → first pane [0, 200), rule [200, 201), grab bar
    // [197.5, 203.5). Press the seam center and drag 100 px right:
    // pointer 300.5 → first = 300 → 0.75.
    ui.press_at(Vec2::new(200.5, 50.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.5, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.75).abs() < 1e-6,
        "pointer 300.5 over span 400 → 0.75, got {ratio}"
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

#[test]
fn divider_requests_the_resize_cursor() {
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    frame_with(&mut ui, &mut ratio);
    frame_with(&mut ui, &mut ratio);
    assert_eq!(ui.cursor, CursorIcon::Default, "idle frame keeps the arrow");

    // Hovering the grab bar ([197.5, 203.5) at ratio 0.5) requests the
    // horizontal-resize cursor.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(200.5, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(ui.cursor, CursorIcon::EwResize, "hover shows resize");

    // Mid-drag the pointer leaves the thin bar; the cursor must hold
    // until release (drag-first, since `hovered` is capture-gated).
    ui.press_at(Vec2::new(200.5, 50.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(320.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(ui.cursor, CursorIcon::EwResize, "drag holds resize off-bar");

    // Release with the pointer over a pane: the per-record-pass reset
    // returns the arrow because nothing re-requests.
    ui.release_left();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(ui.cursor, CursorIcon::Default, "leave resets to the arrow");

    // A vertical splitter's divider asks for the other axis.
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    let frame = |ui: &mut Ui, ratio: &mut f32| {
        ui.run_at_acked(SURFACE, |ui| {
            Splitter::vertical(ratio)
                .id(split_id())
                .size((Sizing::Fixed(100.0), Sizing::Fixed(201.0)))
                .show(ui, |_, _| {});
        });
    };
    frame(&mut ui, &mut ratio);
    frame(&mut ui, &mut ratio);
    // Free span 200 at ratio 0.5 → grab bar rows [97.5, 103.5).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 100.5)));
    frame(&mut ui, &mut ratio);
    assert_eq!(
        ui.cursor,
        CursorIcon::NsResize,
        "column split resizes vertically"
    );
}
