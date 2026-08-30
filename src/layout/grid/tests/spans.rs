//! A cell covering several tracks, and the internal gaps it measures
//! against.

use crate::layout::axis::Axis;

use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::{frame::Frame, grid::Grid};
use crate::widgets::{panel::Panel, text::Text};
use glam::UVec2;

/// A grid reads its spacing from the node column `line_gap` and `gap`
/// write, so the rows and the columns each measure against the value
/// their own setter named.
#[test]
fn grid_span_covers_multiple_tracks_with_gap() {
    // 3 fixed primary tracks of 100 with gap 10 → spanning all = 320.
    // Body sits in track (1,1) → 110 offset on primary, 50 on secondary.
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
                    Frame::new()
                        .id(WidgetId::from_hash("header"))
                        .grid_cell(GridCell::at(0, 0).span(span.0, span.1))
                        .show(ui);
                    Frame::new()
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
                                        .with_line_height_mult(1.0),
                                )
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell(GridCell::at(0, 0).span(1, case.span))
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
                        let panel = match axis {
                            Axis::X => Panel::wrap_hstack(),
                            Axis::Y => Panel::wrap_vstack(),
                        };
                        panel_node = Some(
                            panel
                                .auto_id()
                                .grid_cell(match axis {
                                    Axis::X => GridCell::at(0, 0).span(1, case.span),
                                    Axis::Y => GridCell::at(0, 0).span(case.span, 1),
                                })
                                .show(ui, |ui| {
                                    Frame::new()
                                        .auto_id()
                                        .size(axis.compose_size(case.child_main, 20.0))
                                        .show(ui);
                                    second_node = Some(
                                        Frame::new()
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

/// Pin: 2-D span (row + col) covers the rectangular union with gaps.
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
                Frame::new()
                    .id(WidgetId::from_hash("big"))
                    .grid_cell(GridCell::at(0, 0).span(2, 2))
                    .show(ui);
                Frame::new()
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
