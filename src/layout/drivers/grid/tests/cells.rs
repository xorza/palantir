//! Placing a child in its resolved cell.

use crate::internals::layout_rig::{GridTracks, LayoutRig};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{HAlign, VAlign};
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(200.0, 200.0);

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let tracks = GridTracks {
            rows: &[Track::fixed(100.0)],
            cols: &[Track::fixed(100.0)],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            rec.leaf(
                "pinned",
                Widget::leaf()
                    .grid_cell((0, 0))
                    .size((20.0, 20.0))
                    .align((HAlign::Right, VAlign::Bottom)),
            );
        });
    });
    assert_eq!(rig.rect("pinned"), Rect::new(80.0, 80.0, 20.0, 20.0));
}

/// A cell past the tracks is coerced, not refused: on a 2 × 2 grid of 50 px tracks, `(5, 7)` lands
/// in the last cell, and a span of 3 from row 0 covers the 2 rows there are.
#[test]
fn a_cell_past_the_tracks_is_held_inside_them() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let tracks = GridTracks {
            rows: &[Track::fixed(50.0), Track::fixed(50.0)],
            cols: &[Track::fixed(50.0), Track::fixed(50.0)],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            let fill = || Widget::leaf().size(SizeSpec::FILL);
            rec.leaf("far", fill().grid_cell((5, 7)));
            rec.leaf("tall", fill().grid_cell(GridCell::at(0, 0).with_span(3, 1)));
        });
    });
    assert_eq!(rig.rect("far"), Rect::new(50.0, 50.0, 50.0, 50.0));
    assert_eq!(rig.rect("tall").size, Size::new(50.0, 100.0));
}
