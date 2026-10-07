//! Placing a child in its resolved cell, and the depth stack bracketing the walk.

use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid};
use glam::UVec2;

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    use crate::primitives::layout::align::{Align, HAlign, VAlign};

    let mut h = UiHarness::new(UVec2::new(200, 200));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fixed(100.0)])
            .rows([Track::fixed(100.0)])
            .show(ui, |ui| {
                Block::new()
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

/// Debug-only: `enter`/`exit` pairing is the layout engine's own, so this checks the crate, not a caller.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "GridDepthStack::exit underflow")]
fn grid_depth_stack_rejects_exit_without_enter() {
    use crate::layout::drivers::grid::grid_depth_stack::GridDepthStack;

    GridDepthStack::default().exit();
}
