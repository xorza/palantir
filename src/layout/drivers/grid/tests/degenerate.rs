//! Zero extents, empty dimensions, and a long track list.

use crate::internals::layout_rig::{GridTracks, LayoutRig};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use std::array;

const SURFACE: Size = Size::new(400.0, 400.0);

/// An empty grid (zero rows or cols) measures and arranges to zero without panicking; child rects are zeroed at the parent anchor, and the other dimension's 50 px track doesn't survive.
#[test]
fn grid_empty_dim_measures_to_zero_and_zeros_children() {
    let empty: &[Track] = &[];
    let one: &[Track] = &[Track::fixed(50.0)];
    for (label, cols, rows) in [("no rows", one, empty), ("no cols", empty, one)] {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            let row = Widget::hstack().size((Sizing::FILL, Sizing::FILL));
            rec.node("row", row, |rec| {
                rec.grid(
                    "empty-grid",
                    Widget::grid(),
                    GridTracks { rows, cols },
                    |rec| {
                        rec.leaf("ghost", Widget::leaf().size((20.0, 20.0)));
                    },
                );
            });
        });
        assert_eq!(rig.rect("empty-grid").size, Size::ZERO, "{label}: the grid");
        assert_eq!(rig.rect("ghost").size, Size::ZERO, "{label}: its child");
    }
}

/// A grid whose slot resolves to zero still gives its Fixed track the declared size and its Fill track nothing. `0.0` is a legitimate `resolve_axis` total, so arrange must not read it as "measure never ran": the track sizes it reuses must be the solver's.
#[test]
fn zero_extent_grid_keeps_fixed_track_when_arrange_reuses_the_resolution() {
    let mut rig = LayoutRig::new();
    let mut runs = Vec::new();
    for _ in 0..2 {
        rig.run(SURFACE, |rec| {
            let tracks = GridTracks {
                rows: &[Track::fixed(20.0)],
                cols: &[Track::fixed(30.0), Track::FILL],
            };
            let zero = Widget::grid().size((Sizing::fixed(0.0), Sizing::fixed(0.0)));
            rec.grid("zero-grid", zero, tracks, |rec| {
                rec.leaf("fixed-cell", Widget::leaf().grid_cell((0, 0)));
                rec.leaf("fill-cell", Widget::leaf().grid_cell((0, 1)));
            });
        });
        assert_eq!(rig.rect("zero-grid"), Rect::new(0.0, 0.0, 0.0, 0.0));
        // Phase 1 commits Fixed tracks before sharing leftover, so the zero total leaves the 30×20 cell whole and overflowing its parent (contains-content rule).
        let fixed = rig.rect("fixed-cell");
        assert_eq!(fixed, Rect::new(0.0, 0.0, 30.0, 20.0));
        // Nothing left after the Fixed column, but the row stands.
        let fill = rig.rect("fill-cell");
        assert_eq!(fill, Rect::new(30.0, 0.0, 0.0, 20.0));
        runs.push((fixed, fill));
    }
    assert_eq!(
        runs[0], runs[1],
        "a second run over the same tree must arrange identically",
    );
}

#[test]
fn large_inline_track_definition_has_exact_extent_and_last_cell_position() {
    const COLS: usize = 64;
    let cols: [Track; COLS] = array::from_fn(|i| Track::fixed((i + 1) as f32));
    let mut rig = LayoutRig::new();
    rig.run(Size::new(3_000.0, 100.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::fixed(10.0)],
            cols: &cols,
        };
        rec.grid(
            "large-grid",
            Widget::grid().line_gap(0.0).gap(2.0),
            tracks,
            |rec| {
                rec.leaf(
                    "last-cell",
                    Widget::leaf().grid_cell((0, (COLS - 1) as u16)),
                );
            },
        );
    });

    // Sum 1..=64 = 2,080; 63 gaps × 2 = 126.
    assert_eq!(rig.rect("large-grid").size, Size::new(2_206.0, 10.0));
    // Sum 1..=63 = 2,016; 63 preceding gaps × 2 = 126.
    assert_eq!(rig.rect("last-cell"), Rect::new(2_142.0, 0.0, 64.0, 10.0));
}
