//! A line's cross extent: its max, floors and caps.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::layout::drivers::wrapstack::tests::support::cell;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// Zero-main 30-cross child plus a 20-main child: 0 + 5 + 20 = 25 main, max(30, 10) = 30 cross.
#[test]
fn zero_main_child_still_occupies_the_line_on_both_axes() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        label: &'static str,
        axis: Axis,
        with_normal: bool,
        expected_wrap: Size,
        expected_normal_min: Option<Vec2>,
    }

    let cases = [
        Case {
            label: "horizontal_lone",
            axis: Axis::X,
            with_normal: false,
            expected_wrap: Size::new(0.0, 30.0),
            expected_normal_min: None,
        },
        Case {
            label: "horizontal_followed",
            axis: Axis::X,
            with_normal: true,
            expected_wrap: Size::new(25.0, 30.0),
            expected_normal_min: Some(Vec2::new(5.0, 0.0)),
        },
        Case {
            label: "vertical_lone",
            axis: Axis::Y,
            with_normal: false,
            expected_wrap: Size::new(30.0, 0.0),
            expected_normal_min: None,
        },
        Case {
            label: "vertical_followed",
            axis: Axis::Y,
            with_normal: true,
            expected_wrap: Size::new(30.0, 25.0),
            expected_normal_min: Some(Vec2::new(0.0, 5.0)),
        },
    ];

    for case in cases {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        let zero_size = match case.axis {
            Axis::X => Size::new(0.0, 30.0),
            Axis::Y => Size::new(30.0, 0.0),
        };
        let normal_size = match case.axis {
            Axis::X => Size::new(20.0, 10.0),
            Axis::Y => Size::new(10.0, 20.0),
        };
        h.under_outer(|ui| {
            let panel = Panel::wrap_stack_on(case.axis);
            panel
                .id(WidgetId::from_hash("wrap"))
                .size((Sizing::HUG, Sizing::HUG))
                .gap(5.0)
                .show(ui, |ui| {
                    cell(ui, "zero", zero_size.w, zero_size.h);
                    if case.with_normal {
                        cell(ui, "normal", normal_size.w, normal_size.h);
                    }
                });
        });

        let wrap = h.arranged(WidgetId::from_hash("wrap"));
        assert_eq!(wrap.size, case.expected_wrap, "case: {}", case.label);
        let zero = h.arranged(WidgetId::from_hash("zero"));
        assert_eq!(zero.min, Vec2::ZERO, "case: {} zero origin", case.label);
        assert_eq!(zero.size, zero_size, "case: {} zero size", case.label);
        if let Some(expected) = case.expected_normal_min {
            let normal = h.arranged(WidgetId::from_hash("normal"));
            assert_eq!(normal.min, expected, "case: {} normal origin", case.label);
        }
    }
}

/// Line height is the max child cross; later lines start at the previous bottom + `line_gap`.
#[test]
fn wrap_hstack_line_height_is_max_child_cross() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .gap(0.0)
            .line_gap(0.0)
            .show(ui, |ui| {
                cell(ui, "tall", 100.0, 60.0);
                cell(ui, "short", 100.0, 20.0);
                cell(ui, "next-line", 100.0, 30.0);
            });
    });
    let tall = h.arranged(WidgetId::from_hash("tall"));
    let short = h.arranged(WidgetId::from_hash("short"));
    let next = h.arranged(WidgetId::from_hash("next-line"));
    assert_eq!(tall.min.y, 0.0);
    assert_eq!(short.min.y, 0.0);
    assert_eq!(next.min.y, 60.0);
}

/// Cross-axis `Sizing::fill` stretches to the tallest child, as in Stack.
#[test]
fn wrap_hstack_cross_fill_child_stretches_to_row_height() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(300.0), Sizing::HUG))
            .gap(10.0)
            .show(ui, |ui| {
                cell(ui, "tall", 100.0, 60.0);
                Block::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::fixed(100.0), Sizing::FILL))
                    .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                    .show(ui);
            });
    });
    let tall = h.arranged(WidgetId::from_hash("tall"));
    let filler = h.arranged(WidgetId::from_hash("filler"));
    assert_eq!(tall.size.h, 60.0);
    assert_eq!(
        filler.size.h, 60.0,
        "Fill-on-cross child stretches to row height"
    );
}

#[test]
fn all_fill_lines_preserve_measured_cross_floors_on_both_axes() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        floors: &'static [f32],
        expected_cross_positions: &'static [f32],
        expected_cross_sizes: &'static [f32],
        expected_wrap_cross: f32,
    }

    let cases = [
        Case {
            label: "one_line",
            floors: &[20.0, 30.0],
            expected_cross_positions: &[0.0, 0.0],
            expected_cross_sizes: &[30.0, 30.0],
            expected_wrap_cross: 30.0,
        },
        Case {
            label: "two_lines",
            floors: &[20.0, 30.0, 40.0],
            expected_cross_positions: &[0.0, 0.0, 37.0],
            expected_cross_sizes: &[30.0, 30.0, 40.0],
            expected_wrap_cross: 77.0,
        },
    ];

    for axis in [Axis::X, Axis::Y] {
        for case in &cases {
            let mut h = UiHarness::new(UVec2::new(400, 400));
            h.under_outer(|ui| {
                let panel = Panel::wrap_stack_on(axis);
                panel
                    .id(WidgetId::from_hash("all-fill-wrap"))
                    .size(axis.compose_sizing(Sizing::fixed(125.0), Sizing::HUG))
                    .gap(5.0)
                    .line_gap(7.0)
                    .show(ui, |ui| {
                        for (index, floor) in case.floors.iter().enumerate() {
                            fill_cross_cell(
                                ui,
                                ["fill-a", "fill-b", "fill-c"][index],
                                axis,
                                60.0,
                                *floor,
                            );
                        }
                    });
            });

            let wrap_rect = h.arranged(WidgetId::from_hash("all-fill-wrap"));
            assert_eq!(
                axis.main(wrap_rect.size),
                125.0,
                "{axis:?} {} wrap main",
                case.label
            );
            assert_eq!(
                axis.cross(wrap_rect.size),
                case.expected_wrap_cross,
                "{axis:?} {} wrap cross",
                case.label
            );
            for (index, name) in ["fill-a", "fill-b", "fill-c"]
                .iter()
                .enumerate()
                .take(case.floors.len())
            {
                let rect = h.arranged(WidgetId::from_hash(name));
                assert_eq!(
                    axis.main_v(rect.min),
                    [0.0, 65.0, 0.0][index],
                    "{axis:?} {} child {index} main position",
                    case.label
                );
                assert_eq!(
                    axis.cross_v(rect.min),
                    case.expected_cross_positions[index],
                    "{axis:?} {} child {index} cross position",
                    case.label
                );
                assert_eq!(
                    axis.main(rect.size),
                    60.0,
                    "{axis:?} {} child {index} main size",
                    case.label
                );
                assert_eq!(
                    axis.cross(rect.size),
                    case.expected_cross_sizes[index],
                    "{axis:?} {} child {index} cross size",
                    case.label
                );
            }
        }
    }
}

#[test]
fn fill_floor_can_establish_a_mixed_line_cross_extent() {
    for axis in [Axis::X, Axis::Y] {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        h.under_outer(|ui| {
            let panel = Panel::wrap_stack_on(axis);
            panel
                .id(WidgetId::from_hash("mixed-fill-wrap"))
                .size(axis.compose_sizing(Sizing::fixed(105.0), Sizing::HUG))
                .gap(5.0)
                .line_gap(7.0)
                .show(ui, |ui| {
                    fill_cross_cell(ui, "mixed-fill", axis, 50.0, 40.0);
                    let fixed_size = axis.compose_size(50.0, 20.0);
                    cell(ui, "mixed-fixed", fixed_size.w, fixed_size.h);
                    let next_size = axis.compose_size(50.0, 10.0);
                    cell(ui, "mixed-next", next_size.w, next_size.h);
                });
        });

        let wrap_rect = h.arranged(WidgetId::from_hash("mixed-fill-wrap"));
        assert_eq!(axis.cross(wrap_rect.size), 57.0, "{axis:?} wrap cross");
        let fill = h.arranged(WidgetId::from_hash("mixed-fill"));
        let fixed = h.arranged(WidgetId::from_hash("mixed-fixed"));
        let next = h.arranged(WidgetId::from_hash("mixed-next"));
        assert_eq!(axis.cross(fill.size), 40.0, "{axis:?} fill cross");
        assert_eq!(axis.cross(fixed.size), 20.0, "{axis:?} fixed cross");
        assert_eq!(axis.cross_v(next.min), 47.0, "{axis:?} second line origin");
    }
}

#[test]
fn all_fill_line_cross_floors_respect_explicit_min_and_max() {
    for axis in [Axis::X, Axis::Y] {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        h.under_outer(|ui| {
            let panel = Panel::wrap_stack_on(axis);
            panel
                .id(WidgetId::from_hash("bounded-fill-wrap"))
                .size(axis.compose_sizing(Sizing::fixed(105.0), Sizing::HUG))
                .gap(5.0)
                .line_gap(5.0)
                .show(ui, |ui| {
                    fill_cross_cell(ui, "min-fill", axis, 50.0, 25.0);
                    max_capped_fill_cross_cell(ui, "max-fill", axis, 50.0, 50.0, 35.0);
                    fill_cross_cell(ui, "next-fill", axis, 50.0, 15.0);
                });
        });

        let wrap_rect = h.arranged(WidgetId::from_hash("bounded-fill-wrap"));
        assert_eq!(axis.cross(wrap_rect.size), 55.0, "{axis:?} wrap cross");
        let min_fill = h.arranged(WidgetId::from_hash("min-fill"));
        let max_fill = h.arranged(WidgetId::from_hash("max-fill"));
        let next_fill = h.arranged(WidgetId::from_hash("next-fill"));
        assert_eq!(axis.cross(min_fill.size), 35.0, "{axis:?} min fill");
        assert_eq!(axis.cross(max_fill.size), 35.0, "{axis:?} max fill");
        assert_eq!(
            axis.cross_v(next_fill.min),
            40.0,
            "{axis:?} second line origin"
        );
        assert_eq!(axis.cross(next_fill.size), 15.0, "{axis:?} next fill");
    }
}

fn fill_cross_cell(ui: &mut Ui, id: &'static str, axis: Axis, main: f32, min_cross: f32) -> NodeId {
    Block::new()
        .id(WidgetId::from_hash(id))
        .size(axis.compose_sizing(Sizing::fixed(main), Sizing::FILL))
        .min_size(axis.compose_size(0.0, min_cross))
        .show(ui)
        .node()
}

fn max_capped_fill_cross_cell(
    ui: &mut Ui,
    id: &'static str,
    axis: Axis,
    main: f32,
    content_cross: f32,
    max_cross: f32,
) -> NodeId {
    Panel::zstack()
        .id(WidgetId::from_hash(id))
        .size(axis.compose_sizing(Sizing::fixed(main), Sizing::FILL))
        .max_size(axis.compose_size(f32::INFINITY, max_cross))
        .show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("max-capped-content"))
                .size(axis.compose_sizing(Sizing::fixed(0.0), Sizing::fixed(content_cross)))
                .show(ui);
        })
        .response
        .node()
}
