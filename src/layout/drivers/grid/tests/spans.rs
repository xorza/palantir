//! A cell covering several tracks, and the internal gaps it measures against.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// Rows and columns each measure against their own setter's gap (`line_gap` vs `gap`).
#[test]
fn grid_span_covers_multiple_tracks_with_gap() {
    // 3 fixed tracks of 100 with gap 10: spanning all = 320; a body in track (1,1) sits at 110 primary, 50 secondary.
    for axis in [Axis::X, Axis::Y] {
        let primary = [Track::fixed(100.0); 3];
        let secondary = [Track::fixed(40.0); 2];
        let tracks = match axis {
            Axis::X => GridTracks {
                rows: &secondary,
                cols: &primary,
            },
            Axis::Y => GridTracks {
                rows: &primary,
                cols: &secondary,
            },
        };
        let span = match axis {
            Axis::X => GridCell::at(0, 0).with_span(1, 3),
            Axis::Y => GridCell::at(0, 0).with_span(3, 1),
        };
        let mut rig = LayoutRig::new();
        rig.run(axis.compose_size(400.0, 200.0), |rec| {
            let grid = Widget::grid().line_gap(10.0).gap(10.0);
            rec.grid("grid", grid, tracks, |rec| {
                rec.leaf("header", Widget::leaf().grid_cell(span));
                rec.leaf("body", Widget::leaf().grid_cell((1, 1)));
            });
        });
        let header = rig.rect("header");
        let body = rig.rect("body");
        assert_eq!(axis.main_v(header.min), 0.0, "{axis:?} header main min");
        assert_eq!(
            header.size,
            axis.compose_size(320.0, 40.0),
            "{axis:?} header"
        );
        assert_eq!(axis.main_v(body.min), 110.0, "{axis:?} body main min");
        assert_eq!(axis.cross_v(body.min), 50.0, "{axis:?} body cross min");
        assert_eq!(body.size, axis.compose_size(100.0, 40.0), "{axis:?} body");
    }
}

#[test]
fn spanned_text_measures_against_track_sizes_plus_internal_column_gaps() {
    for case in KNOWN_SPAN_CASES {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(400.0, 200.0), |rec| {
            let cols = fixed_tracks(case);
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols: &cols,
            };
            rec.grid("grid", Widget::grid().gap(case.gap), tracks, |rec| {
                let cell = Widget::leaf().grid_cell(GridCell::at(0, 0).with_span(1, case.span));
                rec.text_leaf(
                    "text",
                    cell,
                    TextRun::new(case.text, 16.0, TextWrap::WrapWithOverflow),
                );
            });
        });
        // The 16 px mono face: 8 px a character, 16 px a line.
        assert_eq!(
            rig.desired("text"),
            Size::new(case.text.len() as f32 * 8.0, 16.0),
            "{} measured text",
            case.label,
        );
        assert_eq!(
            rig.rect("text").size,
            Size::new(case.slot, 16.0),
            "{} arranged text",
            case.label,
        );
        assert_eq!(
            rig.rect("grid").size,
            Size::new(case.slot, 16.0),
            "{} grid extent",
            case.label,
        );
    }
}

#[test]
fn spanned_nested_wrap_measures_against_internal_gaps_on_both_axes() {
    for axis in [Axis::X, Axis::Y] {
        for case in KNOWN_SPAN_CASES {
            let mut rig = LayoutRig::new();
            rig.run(Size::new(400.0, 400.0), |rec| {
                let primary = fixed_tracks(case);
                let secondary = [Track::HUG];
                let (tracks, grid, cell) = match axis {
                    Axis::X => (
                        GridTracks {
                            rows: &secondary,
                            cols: &primary,
                        },
                        Widget::grid().gap(case.gap),
                        GridCell::at(0, 0).with_span(1, case.span),
                    ),
                    Axis::Y => (
                        GridTracks {
                            rows: &primary,
                            cols: &secondary,
                        },
                        Widget::grid().line_gap(case.gap),
                        GridCell::at(0, 0).with_span(case.span, 1),
                    ),
                };
                rec.grid("grid", grid, tracks, |rec| {
                    let panel = Widget::wrap_stack(axis).grid_cell(cell);
                    rec.node("panel", panel, |rec| {
                        for id in ["first", "second"] {
                            let child =
                                Widget::leaf().size(axis.compose_size(case.child_main, 20.0));
                            rec.leaf(id, child);
                        }
                    });
                });
            });

            assert_eq!(
                rig.desired("panel"),
                axis.compose_size(case.child_main * 2.0, 20.0),
                "{axis:?} {} measured panel",
                case.label,
            );
            assert_eq!(
                rig.rect("panel").size,
                axis.compose_size(case.slot, 20.0),
                "{axis:?} {} arranged panel",
                case.label,
            );
            let second = rig.rect("second");
            assert_eq!(
                axis.main_v(second.min),
                case.child_main,
                "{axis:?} {} second child main offset",
                case.label,
            );
            assert_eq!(
                axis.cross_v(second.min),
                0.0,
                "{axis:?} {} second child cross offset",
                case.label,
            );
            assert_eq!(
                second.size,
                axis.compose_size(case.child_main, 20.0),
                "{axis:?} {} second child extent",
                case.label,
            );
        }
    }
}

#[test]
fn grid_cell_with_2d_span_covers_track_union_with_gaps() {
    // 3×3 of fixed-50 cells with gap=10. 2×2 cell at (0,0): w/h = 110.
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 400.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::fixed(50.0); 3],
            cols: &[Track::fixed(50.0); 3],
        };
        rec.grid(
            "grid",
            Widget::grid().line_gap(10.0).gap(10.0),
            tracks,
            |rec| {
                rec.leaf(
                    "big",
                    Widget::leaf().grid_cell(GridCell::at(0, 0).with_span(2, 2)),
                );
                rec.leaf("corner", Widget::leaf().grid_cell((2, 2)));
            },
        );
    });
    let big = rig.rect("big");
    let corner = rig.rect("corner");
    assert_eq!((big.min.x, big.min.y), (0.0, 0.0));
    assert_eq!((big.size.w, big.size.h), (110.0, 110.0));
    assert_eq!((corner.min.x, corner.min.y), (120.0, 120.0));
    assert_eq!((corner.size.w, corner.size.h), (50.0, 50.0));
}

#[derive(Clone, Copy, Debug)]
struct KnownSpanCase {
    label: &'static str,
    span: u16,
    track: f32,
    gap: f32,
    text: &'static str,
    child_main: f32,
    slot: f32,
}

const KNOWN_SPAN_CASES: [KnownSpanCase; 2] = [
    KnownSpanCase {
        label: "two_tracks",
        span: 2,
        track: 40.0,
        gap: 10.0,
        text: "12345 67890",
        child_main: 44.0,
        slot: 90.0,
    },
    KnownSpanCase {
        label: "three_tracks",
        span: 3,
        track: 30.0,
        gap: 5.0,
        text: "12345 678901",
        child_main: 48.0,
        slot: 100.0,
    },
];

fn fixed_tracks(case: KnownSpanCase) -> Vec<Track> {
    (0..case.span).map(|_| Track::fixed(case.track)).collect()
}

/// A fixed-size leaf named `id` in `cell`.
fn block(rec: &mut NodeRecorder<'_>, id: &str, cell: impl Into<GridCell>, size: (f32, f32)) {
    rec.leaf(id, Widget::leaf().grid_cell(cell).size(size));
}

/// A spanning cell grows the Hug tracks it crosses by what their span-1 cells leave short,
/// split evenly (CSS Grid §12.5.1). Columns: 30 and 50 hold `30 + 10 + 50 = 90` of a 200 header,
/// so each takes `110 / 2 = 55`, to 85 and 105. Rows: 20 and 30 hold 50 of a 100 side, to 45 and
/// 55. The grid then holds the spanning cell, where it hugged to 90 and 50 before.
#[test]
fn a_spanning_cell_grows_the_hug_tracks_it_crosses() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 400.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG; 2],
            cols: &[Track::HUG; 2],
        };
        rec.node("column", Widget::vstack(), |rec| {
            rec.grid("cols", Widget::grid().gap(10.0), tracks, |rec| {
                block(
                    rec,
                    "header",
                    GridCell::at(0, 0).with_span(1, 2),
                    (200.0, 10.0),
                );
                block(rec, "cell0", (1, 0), (30.0, 10.0));
                block(rec, "cell1", (1, 1), (50.0, 10.0));
            });
            rec.grid("rows", Widget::grid(), tracks, |rec| {
                block(
                    rec,
                    "side",
                    GridCell::at(0, 0).with_span(2, 1),
                    (10.0, 100.0),
                );
                block(rec, "row0", (0, 1), (10.0, 20.0));
                block(rec, "row1", (1, 1), (10.0, 30.0));
            });
        });
    });
    let cols = rig.rect("cols");
    assert_eq!(cols.size.w, 200.0);
    assert_eq!(
        rig.rect("cell1").min.x - cols.min.x,
        85.0 + 10.0,
        "column 0 is 85 wide"
    );
    let rows = rig.rect("rows");
    assert_eq!(rows.size.h, 100.0);
    assert_eq!(
        rig.rect("row1").min.y - rows.min.y,
        45.0,
        "row 0 is 45 high"
    );
}

/// A spanning cell's share stops at a Hug track's `max`, as the track solve holds it, and the rest
/// goes to the other tracks: of the `110 / 2 = 55` each, the first column takes 10, to its 40, and
/// the second the other 100, to 150, so the grid still holds the 200 header.
#[test]
fn a_spanning_share_stops_at_the_track_max() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 200.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG; 2],
            cols: &[Track::HUG.with_max(40.0), Track::HUG],
        };
        rec.node("column", Widget::vstack(), |rec| {
            rec.grid("grid", Widget::grid().gap(10.0), tracks, |rec| {
                block(
                    rec,
                    "header",
                    GridCell::at(0, 0).with_span(1, 2),
                    (200.0, 10.0),
                );
                block(rec, "cell0", (1, 0), (30.0, 10.0));
                block(rec, "cell1", (1, 1), (50.0, 10.0));
            });
        });
    });
    let grid = rig.rect("grid");
    assert_eq!(
        rig.rect("cell1").min.x - grid.min.x,
        40.0 + 10.0,
        "column 0 holds at its max"
    );
    assert_eq!(grid.size.w, 200.0, "column 1 takes the rest, to 150");
}

/// A spanning cell that crosses a Fill column grows only its Fill columns, by weight, and holds the
/// Hug ones at their own content (CSS Grid §12.5 step 4); such spans grow after every span that
/// crosses no Fill column (steps 3, then 4). Each case: the columns, the spans `(first, count,
/// width)` in record order, a fixed-width cell per column on the second row, and where each column
/// starts. No gaps.
///
/// - `[Hug, Fill, Hug]`, a 300 span over 40 and 40: the Fill takes `300 − 80 = 220`, so the
///   columns start at 0, 40, 260.
/// - `[Fill 1, Hug, Fill 3]`, a 400 span over the 40 middle: `360` splits 1:3, 90 and 270, so
///   the columns start at 0, 90, 130.
/// - `[Hug, Fill max 100, Hug]`, the first case's span: the Fill stops at its 100 and the rest
///   overflows, so the columns start at 0, 40, 140.
/// - `[Hug, Hug, Fill]`, a 150 span over all three recorded before a 100 span over the Hug
///   two: the Hug two take 50 each first, so the Fill takes the 50 left, and the columns start at
///   0, 50, 100. Taken in record order, the Fill would take all 150.
#[test]
fn a_span_over_a_fill_column_grows_only_its_fill_columns() {
    struct Case {
        what: &'static str,
        cols: [Track; 3],
        spans: &'static [(u16, u16, f32)],
        cells: [f32; 3],
        starts: [f32; 3],
    }
    let cases = [
        Case {
            what: "hug fill hug",
            cols: [Track::HUG, Track::FILL, Track::HUG],
            spans: &[(0, 3, 300.0)],
            cells: [40.0, 0.0, 40.0],
            starts: [0.0, 40.0, 260.0],
        },
        Case {
            what: "weighted fills",
            cols: [Track::fill(1.0), Track::HUG, Track::fill(3.0)],
            spans: &[(0, 3, 400.0)],
            cells: [0.0, 40.0, 0.0],
            starts: [0.0, 90.0, 130.0],
        },
        Case {
            what: "fill at its max",
            cols: [Track::HUG, Track::FILL.with_max(100.0), Track::HUG],
            spans: &[(0, 3, 300.0)],
            cells: [40.0, 0.0, 40.0],
            starts: [0.0, 40.0, 140.0],
        },
        Case {
            what: "hug spans first",
            cols: [Track::HUG, Track::HUG, Track::FILL],
            spans: &[(0, 3, 150.0), (0, 2, 100.0)],
            cells: [0.0, 0.0, 0.0],
            starts: [0.0, 50.0, 100.0],
        },
    ];
    for case in cases {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(600.0, 200.0), |rec| {
            let tracks = GridTracks {
                rows: &[Track::HUG; 2],
                cols: &case.cols,
            };
            rec.node("column", Widget::vstack(), |rec| {
                rec.grid("grid", Widget::grid(), tracks, |rec| {
                    for (i, &(first, count, w)) in case.spans.iter().enumerate() {
                        let cell = GridCell::at(0, first).with_span(1, count);
                        block(rec, &format!("span{i}"), cell, (w, 10.0));
                    }
                    for (col, w) in (0u16..).zip(case.cells) {
                        block(rec, &format!("cell{col}"), (1, col), (w, 10.0));
                    }
                });
            });
        });
        let grid = rig.rect("grid");
        let starts = [0, 1, 2].map(|col| rig.rect(&format!("cell{col}")).min.x - grid.min.x);
        assert_eq!(starts, case.starts, "{}", case.what);
    }
}
