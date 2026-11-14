//! How a track resolves: fixed, fill weights, hug, and the floors under each.

use crate::common::span::Span;
use crate::internals::layout_rig::{GridTracks, LayoutRig, TextRun};
use crate::layout::drivers::grid::grid_track_store::{GridAxis, GridAxisId, GridTrackStore};
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::scene::tree::Tree;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use std::num::NonZeroU16;

/// A bare cell: its id, its cell and its size.
type Cell = (&'static str, (u16, u16), (Sizing, Sizing));

/// A Fill grid on a surface of `surface`, over `tracks`, with one bare cell per entry of
/// `cells`: its id, its cell and its size.
fn fill_grid(surface: Size, tracks: GridTracks<'_>, cells: &[Cell]) -> LayoutRig {
    let mut rig = LayoutRig::new();
    rig.run(surface, |rec| {
        let grid = Widget::grid().size(SizeSpec::FILL);
        rec.grid("grid", grid, tracks, |rec| {
            for &(id, cell, size) in cells {
                rec.leaf(id, Widget::leaf().grid_cell(cell).size(size));
            }
        });
    });
    rig
}

const HUG: (Sizing, Sizing) = (Sizing::HUG, Sizing::HUG);

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let tracks = GridTracks {
        rows: &[Track::FILL],
        cols: &[Track::fixed(120.0), Track::FILL],
    };
    let rig = fill_grid(
        Size::new(400.0, 200.0),
        tracks,
        &[("left", (0, 0), HUG), ("right", (0, 1), HUG)],
    );
    assert_eq!(rig.rect("left"), Rect::new(0.0, 0.0, 120.0, 200.0));
    assert_eq!(rig.rect("right"), Rect::new(120.0, 0.0, 280.0, 200.0));
}

/// A Hug column takes its widest span-1 cell: "hello!!" is 7 × 8 = 56 in the 16 px mono face, so
/// the Fill column past it starts at 56 and takes the 344 left of 400.
#[test]
fn grid_hug_column_takes_max_span1_child_intrinsic() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 200.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG, Track::HUG],
            cols: &[Track::HUG, Track::FILL],
        };
        let grid = Widget::grid().size((Sizing::FILL, Sizing::fixed(100.0)));
        rec.grid("grid", grid, tracks, |rec| {
            for (id, text, row) in [("short", "ok", 0), ("long", "hello!!", 1)] {
                rec.text_leaf(
                    id,
                    Widget::leaf().grid_cell((row, 0)),
                    TextRun::new(text, 16.0, TextWrap::SingleLine),
                );
            }
            rec.leaf(
                "body",
                Widget::leaf().grid_cell(
                    GridCell::at(0, 1).with_span(NonZeroU16::new(2).unwrap(), NonZeroU16::MIN),
                ),
            );
        });
    });
    let min_slot = LenReq::MinContent.slot(Axis::X);
    let max_slot = LenReq::MaxContent.slot(Axis::X);
    let cached = |id| rig.engine().scratch.measure.intrinsics[rig.node(id).idx()];
    for id in ["short", "long"] {
        assert!(
            !cached(id)[min_slot].is_nan() && !cached(id)[max_slot].is_nan(),
            "a Hug cell must populate both intrinsic slots in one query",
        );
    }
    assert!(!cached("body")[min_slot].is_nan());
    assert!(
        cached("body")[max_slot].is_nan(),
        "a Fill cell keeps Stack's single min-content path",
    );
    let body = rig.rect("body");
    assert_eq!(body.min.x, 56.0);
    assert_eq!(body.size.w, 344.0);
    assert_eq!(rig.rect("short").min.x, 0.0);
    assert_eq!(rig.rect("long").min.x, 0.0);
}

/// A `Hug` column of `FILL`-width cells hugs to the widest cell's content and every cell stretches to it.
#[test]
fn hug_column_stretches_fill_cells_to_widest_content() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 200.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG, Track::HUG],
            cols: &[Track::HUG],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            for (id, row, w) in [("a", 0, 120.0), ("b", 1, 60.0)] {
                let cell = Widget::hstack().grid_cell((row, 0)).size(SizeSpec::FILL_W);
                rec.node(id, cell, |rec| {
                    rec.leaf(&format!("f{id}"), Widget::leaf().size((w, 20.0)));
                });
            }
        });
    });
    assert_eq!(
        rig.rect("a").size.w,
        120.0,
        "column hugs to the widest cell's content"
    );
    assert_eq!(
        rig.rect("b").size.w,
        120.0,
        "the narrow FILL cell stretched to match"
    );
}

/// A `.max()` on a `Hug` column caps the track: shrinkable content follows, Fixed content
/// keeps its exact extent and overflows.
#[test]
fn hug_column_max_caps_shrinkable_and_rigid_content() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(600.0, 200.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG],
            cols: &[Track::HUG.with_max(150.0)],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            let cell = Widget::leaf().grid_cell((0, 0)).size(SizeSpec::FILL_W);
            rec.text_leaf(
                "label",
                cell,
                TextRun::new("a_very_long_value_label_here", 16.0, TextWrap::Ellipsis),
            );
        });
    });
    assert_eq!(
        rig.rect("label").size.w,
        150.0,
        "hug column capped at its max"
    );

    let rigid = rigid_first_col(Track::HUG.with_max(150.0), 100.0);
    assert_eq!(
        rigid.rect("rigid").size.w,
        200.0,
        "Fixed child remains exact"
    );
    assert_eq!(
        rigid.rect("flex").min.x,
        150.0,
        "the next track starts after the capped Hug track",
    );
}

#[test]
fn grid_fill_weights_and_clamps() {
    type Case = (
        &'static str,
        Track,
        Track,
        f32, // expected col 0 w
        f32, // expected col 1 w
    );
    let cases: &[Case] = &[
        (
            "weights_split_proportionally",
            Track::fill(1.0),
            Track::fill(3.0),
            100.0,
            300.0,
        ),
        (
            "min_clamp_steals_from_other_stars",
            Track::fill(1.0).with_min(200.0),
            Track::fill(3.0),
            200.0,
            200.0,
        ),
        (
            "max_clamp_donates_to_other_stars",
            Track::fill(3.0).with_max(150.0),
            Track::fill(1.0),
            150.0,
            250.0,
        ),
        (
            "maximum_finite_weights",
            Track::fill(f32::MAX),
            Track::fill(f32::MAX),
            200.0,
            200.0,
        ),
    ];
    for (label, c0, c1, want0, want1) in cases {
        let tracks = GridTracks {
            rows: &[Track::FILL],
            cols: &[*c0, *c1],
        };
        let rig = fill_grid(
            Size::new(400.0, 100.0),
            tracks,
            &[("a", (0, 0), HUG), ("b", (0, 1), HUG)],
        );
        assert_eq!(rig.rect("a").size.w, *want0, "case: {label} col0");
        assert_eq!(rig.rect("b").size.w, *want1, "case: {label} col1");
    }

    // Col 0 caps at 100px and donates 300px to col 1; its Fixed(200) child overflows without changing the tracks.
    let rigid = rigid_first_col(Track::FILL.with_max(100.0), 400.0);
    assert_eq!(
        rigid.rect("rigid").size.w,
        200.0,
        "Fixed child remains exact"
    );
    assert_eq!(
        rigid.rect("flex").min.x,
        100.0,
        "col 0 track is capped at 100px"
    );
    assert_eq!(rigid.rect("flex").size.w, 300.0, "col 1 receives 400 - 100");
}

#[test]
fn grid_fill_col_floors_at_descendant_min_content() {
    // Two Fill cols in 300px; col 0 holds a Fixed 200 frame, its MinContent floor: without it they split
    // 150/150 and the frame overflows; with it col 0 clamps to 200, col 1 takes 100.
    let tracks = GridTracks {
        rows: &[Track::FILL],
        cols: &[Track::FILL, Track::FILL],
    };
    let rig = fill_grid(
        Size::new(300.0, 100.0),
        tracks,
        &[
            ("rigid", (0, 0), (Sizing::fixed(200.0), Sizing::FILL)),
            ("flex", (0, 1), HUG),
        ],
    );
    assert_eq!(
        rig.rect("rigid").size.w,
        200.0,
        "rigid cell floors at descendant min-content"
    );
    assert_eq!(
        rig.rect("flex").size.w,
        100.0,
        "flex cell takes the remainder"
    );
}

#[test]
fn grid_fill_row_floors_at_descendant_min_content() {
    // Y twin: two Fill rows in 100px, row 0 holding a Fixed 60 frame; its Y min-content (the row range measure
    // writes) floors row 0 at 60, leaving 40 for row 1, instead of a 50/50 split.
    let tracks = GridTracks {
        rows: &[Track::FILL, Track::FILL],
        cols: &[Track::FILL],
    };
    let rig = fill_grid(
        Size::new(100.0, 100.0),
        tracks,
        &[
            ("rigid", (0, 0), (Sizing::FILL, Sizing::fixed(60.0))),
            ("flex", (1, 0), HUG),
        ],
    );
    assert_eq!(
        rig.rect("rigid").size.h,
        60.0,
        "rigid row floors at descendant min-content"
    );
    assert_eq!(
        rig.rect("flex").size.h,
        40.0,
        "flex row takes the remainder"
    );
}

#[test]
fn grid_hug_rows_floor_at_their_measured_height_when_cramped() {
    // Two Hug rows of Fixed 60 frames in a 100px grid: each range is `[60, 60]`, `sum_hug_min` 120 > 100,
    // so the cramped arm keeps both at 60 and the grid overflows by 20 (an unwritten min gave 50 per row).
    let mut rig = LayoutRig::new();
    rig.run(Size::new(100.0, 100.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG, Track::HUG],
            cols: &[Track::FILL],
        };
        let grid = Widget::grid().size((Sizing::FILL, Sizing::fixed(100.0)));
        rec.grid("grid", grid, tracks, |rec| {
            for (id, row) in [("top", 0), ("bottom", 1)] {
                let cell = Widget::leaf()
                    .grid_cell((row, 0))
                    .size((Sizing::FILL, Sizing::fixed(60.0)));
                rec.leaf(id, cell);
            }
        });
    });
    assert_eq!(
        rig.rect("top").size.h,
        60.0,
        "row 0 keeps its measured height"
    );
    let bottom = rig.rect("bottom");
    assert_eq!(
        bottom.min.y, 60.0,
        "row 1 starts past row 0's full height, so the grid overflows"
    );
    assert_eq!(bottom.size.h, 60.0, "row 1 keeps its measured height");
}

/// `Fixed`/`Hug` resolve; `Fill` stays unresolved so cells see `INF` (WPF defers Fill to arrange).
/// The solve notes the total it ran against, which arrange's reuse reads.
#[test]
fn resolve_axis_marks_fixed_and_hug_resolved_but_leaves_fill_unresolved() {
    let tracks = [Track::fixed(50.0), Track::HUG, Track::FILL];
    let mut tree = Tree::default();
    let def = tree.push_grid_def(&[Track::HUG], &tracks);
    let mut store = GridTrackStore::default();
    store.reset_for(&tree);
    let id = GridAxisId { def, axis: Axis::X };
    let ranges = store.ranges_mut(id);
    ranges.min.copy_from_slice(&[0.0, 10.0, 0.0]);
    ranges.max.copy_from_slice(&[0.0, 30.0, 0.0]);
    let axis = GridAxis {
        id,
        tracks: &tracks,
        gap: 0.0,
    };
    store.resolve_axis(axis, 200.0);

    // 200 - 50 fixed leaves 150; the Hug takes its 30, the Fill the other 120.
    assert_eq!(store.sizes(id), &[50.0, 30.0, 120.0]);
    assert_eq!(
        store.known_span_size(id, Span::new(0, 2), 10.0),
        50.0 + 10.0 + 30.0,
        "the fully resolved Fixed + Hug span includes its internal gap",
    );
    assert!(
        store
            .known_span_size(id, Span::new(1, 2), 10.0)
            .is_infinite(),
        "a span containing unresolved Fill remains uncommitted, so its cells see INF",
    );
    store.commit_fill(id, &tracks);
    assert_eq!(
        store.known_span_size(id, Span::new(2, 1), 10.0),
        120.0,
        "a committed Fill track is known",
    );
}

/// Each Hug row resolves to its own cells' max desired height, independent of other rows.
#[test]
fn grid_multi_row_hug_heights_resolve_independently() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 400.0), |rec| {
        rec.node("row", Widget::hstack().size(SizeSpec::FILL), |rec| {
            let tracks = GridTracks {
                rows: &[Track::HUG; 3],
                cols: &[Track::fixed(50.0)],
            };
            rec.grid("multi-row", Widget::grid(), tracks, |rec| {
                for (id, row, h) in [("short", 0, 10.0), ("tall", 1, 80.0), ("med", 2, 30.0)] {
                    rec.leaf(id, Widget::leaf().size((50.0, h)).grid_cell((row, 0)));
                }
            });
        });
    });
    assert_eq!(rig.rect("short").size.h, 10.0);
    assert_eq!(rig.rect("tall").size.h, 80.0);
    assert_eq!(rig.rect("med").size.h, 30.0);
    assert_eq!(rig.rect("multi-row").size.h, 120.0);
}

/// A Fill grid of `first` and a Fill column, on a surface `surface_w` wide: a Fixed 200 cell
/// `rigid` in `first`, a bare cell `flex` past it.
fn rigid_first_col(first: Track, surface_w: f32) -> LayoutRig {
    let tracks = GridTracks {
        rows: &[Track::FILL],
        cols: &[first, Track::FILL],
    };
    fill_grid(
        Size::new(surface_w, 100.0),
        tracks,
        &[
            ("rigid", (0, 0), (Sizing::fixed(200.0), Sizing::FILL)),
            ("flex", (0, 1), HUG),
        ],
    )
}
