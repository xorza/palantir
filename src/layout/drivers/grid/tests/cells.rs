//! Placing a child in its resolved cell, and the depth stack bracketing the walk.

use crate::internals::ui_harness::UiHarness;
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

/// A cell past the tracks is coerced, not refused: on a 2 × 2 grid of 50 px tracks, `(5, 7)` lands
/// in the last cell, and a span of 3 from row 0 covers the 2 rows there are. Before, a debug build
/// panicked at record time and a release build in layout.
#[test]
fn a_cell_past_the_tracks_is_held_inside_them() {
    use crate::primitives::layout::grid_cell::GridCell;
    use crate::primitives::layout::sizing::Sizing;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Grid::new()
            .id(WidgetId::from_hash("grid"))
            .cols([Track::fixed(50.0), Track::fixed(50.0)])
            .rows([Track::fixed(50.0), Track::fixed(50.0)])
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("far"))
                    .grid_cell((5, 7))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("tall"))
                    .grid_cell(GridCell::at(0, 0).with_span(3, 1))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            });
    });
    let grid = h.arranged(WidgetId::from_hash("grid"));
    let far = h.arranged(WidgetId::from_hash("far"));
    assert_eq!(far.min - grid.min, glam::Vec2::new(50.0, 50.0));
    assert_eq!((far.size.w, far.size.h), (50.0, 50.0));
    let tall = h.arranged(WidgetId::from_hash("tall"));
    assert_eq!((tall.size.w, tall.size.h), (50.0, 100.0));
}
