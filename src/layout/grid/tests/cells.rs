//! Placing a child inside its resolved cell, and the depth stack that
//! brackets the walk.

use crate::layout::grid::grid_depth_stack::GridDepthStack;
use crate::layout::types::track::Track;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, grid::Grid};
use glam::UVec2;

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    use crate::layout::types::{align::Align, align::HAlign, align::VAlign};

    let mut h = UiHarness::new(UVec2::new(200, 200));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fixed(100.0)])
            .rows([Track::fixed(100.0)])
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("pinned"))
                    .grid_cell((0, 0))
                    .size((20.0, 20.0))
                    .align(Align::new(HAlign::Right, VAlign::Bottom))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.main_child_rects(root)[0];
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    assert_eq!(r.min.x, 80.0);
    assert_eq!(r.min.y, 80.0);
}

/// Debug-only: `enter`/`exit` are the layout engine's own pairing, run
/// per grid node per frame, so this is the crate checking itself rather
/// than screening anything a caller passed.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "GridDepthStack::exit underflow")]
fn grid_depth_stack_rejects_exit_without_enter() {
    GridDepthStack::default().exit();
}
