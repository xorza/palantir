//! A cell covering several tracks, and the internal gaps it measures against.

use crate::primitives::layout::axis::Axis;

use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::scene::layer::Layer;
use crate::text::wrap::TextWrap;
use crate::theme::text_style::TextStyle;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid};
use crate::widgets::{panel::Panel, text::Text};
use glam::UVec2;

/// Rows and columns each measure against their own setter's gap (`line_gap` vs `gap`).
#[test]
fn grid_span_covers_multiple_tracks_with_gap() {
    // 3 fixed tracks of 100 with gap 10: spanning all = 320; a body in track (1,1) sits at 110 primary, 50 secondary.
    let cases: &[(&str, bool)] = &[("col_span", false), ("row_span", true)];
    for (label, swap) in cases {
        let surface = if *swap {
            UVec2::new(200, 400)
        } else {
            UVec2::new(400, 200)
        };
        let mut h = UiHarness::new(surface);
        let root = h.frame_value(|ui| {
            let primary = [
                Track::fixed(100.0),
                Track::fixed(100.0),
                Track::fixed(100.0),
            ];
            let secondary = [Track::fixed(40.0), Track::fixed(40.0)];
            let (rows, cols): (&[Track], &[Track]) = if *swap {
                (&primary, &secondary)
            } else {
                (&secondary, &primary)
            };
            let span = if *swap { (3, 1) } else { (1, 3) };
            Grid::new()
                .auto_id()
                .line_gap(10.0)
                .gap(10.0)
                .rows(rows)
                .cols(cols)
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("header"))
                        .grid_cell(GridCell::at(0, 0).with_span(span.0, span.1))
                        .show(ui);
                    Block::new()
                        .id(WidgetId::from_hash("body"))
                        .grid_cell((1, 1))
                        .show(ui);
                })
                .response
                .node()
        });
        let kids = h.main_child_rects(root);
        let header = kids[0];
        let body = kids[1];
        let (h_pri_min, h_pri_size, h_sec_size) = if *swap {
            (header.min.y, header.size.h, header.size.w)
        } else {
            (header.min.x, header.size.w, header.size.h)
        };
        let (b_pri_min, b_sec_min, b_pri_size, b_sec_size) = if *swap {
            (body.min.y, body.min.x, body.size.h, body.size.w)
        } else {
            (body.min.x, body.min.y, body.size.w, body.size.h)
        };
        assert_eq!(h_pri_min, 0.0, "case: {label} header pri_min");
        assert_eq!(h_pri_size, 320.0, "case: {label} header pri_size");
        assert_eq!(h_sec_size, 40.0, "case: {label} header sec_size");
        assert_eq!(b_pri_min, 110.0, "case: {label} body pri_min");
        assert_eq!(b_sec_min, 50.0, "case: {label} body sec_min");
        assert_eq!(b_pri_size, 100.0, "case: {label} body pri_size");
        assert_eq!(b_sec_size, 40.0, "case: {label} body sec_size");
    }
}

#[test]
fn spanned_text_measures_against_track_sizes_plus_internal_column_gaps() {
    for case in KNOWN_SPAN_CASES {
        let mut h = UiHarness::new(UVec2::new(400, 200));
        let mut grid_node = None;
        let mut text_node = None;
        h.frame(|ui| {
            grid_node = Some(
                Grid::new()
                    .auto_id()
                    .cols(fixed_tracks(case))
                    .rows([Track::HUG])
                    .line_gap(0.0)
                    .gap(case.gap)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        text_node = Some(
                            Text::new(case.text)
                                .auto_id()
                                .style(
                                    &TextStyle::default()
                                        .with_font_size(16.0)
                                        .with_line_height_factor(1.0),
                                )
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell(GridCell::at(0, 0).with_span(1, case.span))
                                .show(ui)
                                .node(),
                        );
                    })
                    .response
                    .node(),
            );
        });

        let text = text_node.unwrap();
        let desired = h.engines.layout.cache.captured_desired()[text.idx()];
        assert_eq!(
            desired,
            Size::new(case.text.len() as f32 * 8.0, 16.0),
            "{} measured text",
            case.label,
        );
        assert_eq!(
            h.ui.arranged_rect(Layer::Main, text).size,
            Size::new(case.slot, 16.0),
            "{} arranged text",
            case.label,
        );
        assert_eq!(
            h.ui.arranged_rect(Layer::Main, grid_node.unwrap()).size,
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
            let mut h = UiHarness::new(UVec2::new(400, 400));
            let mut panel_node = None;
            let mut second_node = None;
            h.frame(|ui| {
                let primary = fixed_tracks(case);
                let secondary = [Track::HUG];
                let (rows, cols): (&[Track], &[Track]) = if axis == Axis::X {
                    (&secondary, primary.as_ref())
                } else {
                    (primary.as_ref(), &secondary)
                };
                Grid::new()
                    .auto_id()
                    .rows(rows)
                    .cols(cols)
                    .line_gap(if axis == Axis::Y { case.gap } else { 0.0 })
                    .gap(if axis == Axis::X { case.gap } else { 0.0 })
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        let panel = Panel::wrap_stack_on(axis);
                        panel_node = Some(
                            panel
                                .auto_id()
                                .grid_cell(match axis {
                                    Axis::X => GridCell::at(0, 0).with_span(1, case.span),
                                    Axis::Y => GridCell::at(0, 0).with_span(case.span, 1),
                                })
                                .show(ui, |ui| {
                                    Block::new()
                                        .auto_id()
                                        .size(axis.compose_size(case.child_main, 20.0))
                                        .show(ui);
                                    second_node = Some(
                                        Block::new()
                                            .auto_id()
                                            .size(axis.compose_size(case.child_main, 20.0))
                                            .show(ui)
                                            .node(),
                                    );
                                })
                                .response
                                .node(),
                        );
                    });
            });

            let panel = panel_node.unwrap();
            let expected_desired = axis.compose_size(case.child_main * 2.0, 20.0);
            assert_eq!(
                h.engines.layout.cache.captured_desired()[panel.idx()],
                expected_desired,
                "{axis:?} {} measured panel",
                case.label,
            );
            assert_eq!(
                h.ui.arranged_rect(Layer::Main, panel).size,
                axis.compose_size(case.slot, 20.0),
                "{axis:?} {} arranged panel",
                case.label,
            );
            let second = h.ui.arranged_rect(Layer::Main, second_node.unwrap());
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
    let mut h = UiHarness::new(UVec2::new(400, 400));
    // 3×3 of fixed-50 cells with gap=10. 2×2 cell at (0,0): w/h = 110.
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
            .rows([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
            .line_gap(10.0)
            .gap(10.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("big"))
                    .grid_cell(GridCell::at(0, 0).with_span(2, 2))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("corner"))
                    .grid_cell((2, 2))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    let big = kids[0];
    let corner = kids[1];

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

/// A spanning cell grows the Hug tracks it crosses by what their span-1 cells leave short,
/// split evenly (CSS Grid §12.5.1). Columns: 30 and 50 hold `30 + 10 + 50 = 90` of a 200 header,
/// so each takes `110 / 2 = 55`, to 85 and 105. Rows: 20 and 30 hold 50 of a 100 side, to 45 and
/// 55. The grid then holds the spanning cell, where it hugged to 90 and 50 before.
#[test]
fn a_spanning_cell_grows_the_hug_tracks_it_crosses() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Grid::new()
                .id(WidgetId::from_hash("cols"))
                .size((Sizing::HUG, Sizing::HUG))
                .cols([Track::HUG, Track::HUG])
                .rows([Track::HUG, Track::HUG])
                .gap(10.0)
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("header"))
                        .grid_cell(GridCell::at(0, 0).with_span(1, 2))
                        .size((Sizing::fixed(200.0), Sizing::fixed(10.0)))
                        .show(ui);
                    for (col, w) in [(0, 30.0), (1, 50.0)] {
                        Block::new()
                            .id(WidgetId::from_hash(("cell", col)))
                            .grid_cell((1, col))
                            .size((Sizing::fixed(w), Sizing::fixed(10.0)))
                            .show(ui);
                    }
                });
            Grid::new()
                .id(WidgetId::from_hash("rows"))
                .size((Sizing::HUG, Sizing::HUG))
                .cols([Track::HUG, Track::HUG])
                .rows([Track::HUG, Track::HUG])
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("side"))
                        .grid_cell(GridCell::at(0, 0).with_span(2, 1))
                        .size((Sizing::fixed(10.0), Sizing::fixed(100.0)))
                        .show(ui);
                    for (row, ht) in [(0, 20.0), (1, 30.0)] {
                        Block::new()
                            .id(WidgetId::from_hash(("row-cell", row)))
                            .grid_cell((row, 1))
                            .size((Sizing::fixed(10.0), Sizing::fixed(ht)))
                            .show(ui);
                    }
                });
        });
    });
    let cols = h.arranged(WidgetId::from_hash("cols"));
    assert_eq!(cols.size.w, 200.0);
    let second = h.arranged(WidgetId::from_hash(("cell", 1)));
    assert_eq!(
        second.min.x - cols.min.x,
        85.0 + 10.0,
        "column 0 is 85 wide"
    );
    let rows = h.arranged(WidgetId::from_hash("rows"));
    assert_eq!(rows.size.h, 100.0);
    let lower = h.arranged(WidgetId::from_hash(("row-cell", 1)));
    assert_eq!(lower.min.y - rows.min.y, 45.0, "row 0 is 45 high");
}

/// A spanning cell's share stops at a Hug track's `max`, as the track solve holds it, and the rest
/// goes to the other tracks: of the `110 / 2 = 55` each, the first column takes 10, to its 40, and
/// the second the other 100, to 150, so the grid still holds the 200 header.
#[test]
fn a_spanning_share_stops_at_the_track_max() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Grid::new()
                .id(WidgetId::from_hash("grid"))
                .size((Sizing::HUG, Sizing::HUG))
                .cols([Track::HUG.with_max(40.0), Track::HUG])
                .rows([Track::HUG, Track::HUG])
                .gap(10.0)
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("header"))
                        .grid_cell(GridCell::at(0, 0).with_span(1, 2))
                        .size((Sizing::fixed(200.0), Sizing::fixed(10.0)))
                        .show(ui);
                    for (col, w) in [(0, 30.0), (1, 50.0)] {
                        Block::new()
                            .id(WidgetId::from_hash(("cell", col)))
                            .grid_cell((1, col))
                            .size((Sizing::fixed(w), Sizing::fixed(10.0)))
                            .show(ui);
                    }
                });
        });
    });
    let grid = h.arranged(WidgetId::from_hash("grid"));
    let second = h.arranged(WidgetId::from_hash(("cell", 1)));
    assert_eq!(
        second.min.x - grid.min.x,
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
        let mut h = UiHarness::new(UVec2::new(600, 200));
        h.frame(|ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("grid"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .cols(case.cols)
                    .rows([Track::HUG, Track::HUG])
                    .show(ui, |ui| {
                        for (i, &(first, count, w)) in case.spans.iter().enumerate() {
                            Block::new()
                                .id(WidgetId::from_hash(("span", i)))
                                .grid_cell(GridCell::at(0, first).with_span(1, count))
                                .size((Sizing::fixed(w), Sizing::fixed(10.0)))
                                .show(ui);
                        }
                        for (col, w) in (0u16..).zip(case.cells) {
                            Block::new()
                                .id(WidgetId::from_hash(("cell", col)))
                                .grid_cell((1, col))
                                .size((Sizing::fixed(w), Sizing::fixed(10.0)))
                                .show(ui);
                        }
                    });
            });
        });
        let grid = h.arranged(WidgetId::from_hash("grid"));
        let starts = [0u16, 1, 2]
            .map(|col| h.arranged(WidgetId::from_hash(("cell", col))).min.x - grid.min.x);
        assert_eq!(starts, case.starts, "{}", case.what);
    }
}
