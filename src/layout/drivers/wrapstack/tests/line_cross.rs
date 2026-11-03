//! A line's cross extent: its max, floors and caps.

use crate::internals::layout_rig::{LayoutRig, NodeRecorder};
use crate::layout::drivers::wrapstack::tests::support::{cell, outer};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use glam::Vec2;

const SURFACE: Size = Size::new(400.0, 400.0);

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
        let zero_size = case.axis.compose_size(0.0, 30.0);
        let normal_size = case.axis.compose_size(20.0, 10.0);
        let mut rig = LayoutRig::new();
        rig.run(Size::new(200.0, 200.0), |rec| {
            outer(rec, |rec| {
                rec.node("wrap", Widget::wrap_stack(case.axis).gap(5.0), |rec| {
                    cell(rec, "zero", zero_size);
                    if case.with_normal {
                        cell(rec, "normal", normal_size);
                    }
                });
            });
        });

        assert_eq!(
            rig.rect("wrap").size,
            case.expected_wrap,
            "case: {}",
            case.label
        );
        let zero = rig.rect("zero");
        assert_eq!(zero.min, Vec2::ZERO, "case: {} zero origin", case.label);
        assert_eq!(zero.size, zero_size, "case: {} zero size", case.label);
        if let Some(expected) = case.expected_normal_min {
            assert_eq!(
                rig.rect("normal").min,
                expected,
                "case: {} normal origin",
                case.label
            );
        }
    }
}

/// Line height is the max child cross; later lines start at the previous bottom + `line_gap`.
#[test]
fn wrap_hstack_line_height_is_max_child_cross() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack().size((Sizing::fixed(200.0), Sizing::HUG));
            rec.node("w", wrap, |rec| {
                cell(rec, "tall", Size::new(100.0, 60.0));
                cell(rec, "short", Size::new(100.0, 20.0));
                cell(rec, "next-line", Size::new(100.0, 30.0));
            });
        });
    });
    assert_eq!(rig.rect("tall").min.y, 0.0);
    assert_eq!(rig.rect("short").min.y, 0.0);
    assert_eq!(rig.rect("next-line").min.y, 60.0);
}

/// Cross-axis `Sizing::fill` stretches to the tallest child, as in Stack.
#[test]
fn wrap_hstack_cross_fill_child_stretches_to_row_height() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack()
                .size((Sizing::fixed(300.0), Sizing::HUG))
                .gap(10.0);
            rec.node("w", wrap, |rec| {
                cell(rec, "tall", Size::new(100.0, 60.0));
                rec.leaf(
                    "filler",
                    Widget::leaf().size((Sizing::fixed(100.0), Sizing::FILL)),
                );
            });
        });
    });
    assert_eq!(rig.rect("tall").size.h, 60.0);
    assert_eq!(
        rig.rect("filler").size.h,
        60.0,
        "Fill-on-cross child stretches to row height"
    );
}

#[test]
fn all_fill_lines_preserve_measured_cross_floors_on_both_axes() {
    const IDS: [&str; 3] = ["fill-a", "fill-b", "fill-c"];

    #[derive(Debug)]
    struct Case {
        label: &'static str,
        floors: &'static [f32],
        expected_cross_poss: &'static [f32],
        expected_cross_sizes: &'static [f32],
        expected_wrap_cross: f32,
    }

    let cases = [
        Case {
            label: "one_line",
            floors: &[20.0, 30.0],
            expected_cross_poss: &[0.0, 0.0],
            expected_cross_sizes: &[30.0, 30.0],
            expected_wrap_cross: 30.0,
        },
        Case {
            label: "two_lines",
            floors: &[20.0, 30.0, 40.0],
            expected_cross_poss: &[0.0, 0.0, 37.0],
            expected_cross_sizes: &[30.0, 30.0, 40.0],
            expected_wrap_cross: 77.0,
        },
    ];

    for axis in [Axis::X, Axis::Y] {
        for case in &cases {
            let mut rig = LayoutRig::new();
            rig.run(SURFACE, |rec| {
                outer(rec, |rec| {
                    let wrap = Widget::wrap_stack(axis)
                        .size(axis.compose_sizing(Sizing::fixed(125.0), Sizing::HUG))
                        .gap(5.0)
                        .line_gap(7.0);
                    rec.node("all-fill-wrap", wrap, |rec| {
                        for (id, floor) in IDS.iter().zip(case.floors) {
                            FillCrossCell {
                                id,
                                axis,
                                main: 60.0,
                            }
                            .floored(rec, *floor);
                        }
                    });
                });
            });

            let wrap = rig.rect("all-fill-wrap");
            assert_eq!(
                axis.main(wrap.size),
                125.0,
                "{axis:?} {} wrap main",
                case.label
            );
            assert_eq!(
                axis.cross(wrap.size),
                case.expected_wrap_cross,
                "{axis:?} {} wrap cross",
                case.label
            );
            for (idx, id) in IDS.iter().enumerate().take(case.floors.len()) {
                let rect = rig.rect(id);
                assert_eq!(
                    axis.main_v(rect.min),
                    [0.0, 65.0, 0.0][idx],
                    "{axis:?} {} child {idx} main position",
                    case.label
                );
                assert_eq!(
                    axis.cross_v(rect.min),
                    case.expected_cross_poss[idx],
                    "{axis:?} {} child {idx} cross position",
                    case.label
                );
                assert_eq!(
                    axis.main(rect.size),
                    60.0,
                    "{axis:?} {} child {idx} main size",
                    case.label
                );
                assert_eq!(
                    axis.cross(rect.size),
                    case.expected_cross_sizes[idx],
                    "{axis:?} {} child {idx} cross size",
                    case.label
                );
            }
        }
    }
}

#[test]
fn fill_floor_can_establish_a_mixed_line_cross_extent() {
    for axis in [Axis::X, Axis::Y] {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            outer(rec, |rec| {
                let wrap = Widget::wrap_stack(axis)
                    .size(axis.compose_sizing(Sizing::fixed(105.0), Sizing::HUG))
                    .gap(5.0)
                    .line_gap(7.0);
                rec.node("mixed-fill-wrap", wrap, |rec| {
                    FillCrossCell {
                        id: "mixed-fill",
                        axis,
                        main: 50.0,
                    }
                    .floored(rec, 40.0);
                    cell(rec, "mixed-fixed", axis.compose_size(50.0, 20.0));
                    cell(rec, "mixed-next", axis.compose_size(50.0, 10.0));
                });
            });
        });

        assert_eq!(
            axis.cross(rig.rect("mixed-fill-wrap").size),
            57.0,
            "{axis:?} wrap cross"
        );
        assert_eq!(
            axis.cross(rig.rect("mixed-fill").size),
            40.0,
            "{axis:?} fill cross"
        );
        assert_eq!(
            axis.cross(rig.rect("mixed-fixed").size),
            20.0,
            "{axis:?} fixed cross"
        );
        assert_eq!(
            axis.cross_v(rig.rect("mixed-next").min),
            47.0,
            "{axis:?} second line origin"
        );
    }
}

#[test]
fn all_fill_line_cross_floors_respect_explicit_min_and_max() {
    for axis in [Axis::X, Axis::Y] {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            outer(rec, |rec| {
                let wrap = Widget::wrap_stack(axis)
                    .size(axis.compose_sizing(Sizing::fixed(105.0), Sizing::HUG))
                    .gap(5.0)
                    .line_gap(5.0);
                rec.node("bounded-fill-wrap", wrap, |rec| {
                    let fill = |id| FillCrossCell {
                        id,
                        axis,
                        main: 50.0,
                    };
                    fill("min-fill").floored(rec, 25.0);
                    fill("max-fill").capped(rec, 50.0, 35.0);
                    fill("next-fill").floored(rec, 15.0);
                });
            });
        });

        assert_eq!(
            axis.cross(rig.rect("bounded-fill-wrap").size),
            55.0,
            "{axis:?} wrap cross"
        );
        assert_eq!(
            axis.cross(rig.rect("min-fill").size),
            35.0,
            "{axis:?} min fill"
        );
        assert_eq!(
            axis.cross(rig.rect("max-fill").size),
            35.0,
            "{axis:?} max fill"
        );
        let next = rig.rect("next-fill");
        assert_eq!(axis.cross_v(next.min), 40.0, "{axis:?} second line origin");
        assert_eq!(axis.cross(next.size), 15.0, "{axis:?} next fill");
    }
}

/// A cell fixed at `main` on `axis` that fills its line's cross extent.
#[derive(Clone, Copy, Debug)]
struct FillCrossCell {
    id: &'static str,
    axis: Axis,
    main: f32,
}

impl FillCrossCell {
    fn widget(self) -> Widget {
        Widget::leaf().size(
            self.axis
                .compose_sizing(Sizing::fixed(self.main), Sizing::FILL),
        )
    }

    /// Floored at `min_cross` across.
    fn floored(self, rec: &mut NodeRecorder<'_>, min_cross: f32) {
        let floored = self
            .widget()
            .min_size(self.axis.compose_size(0.0, min_cross));
        rec.leaf(self.id, floored);
    }

    /// Holding content `content_cross` across, capped at `max_cross`.
    fn capped(self, rec: &mut NodeRecorder<'_>, content_cross: f32, max_cross: f32) {
        let Self { id, axis, .. } = self;
        let capped = Widget::zstack()
            .size(axis.compose_sizing(Sizing::fixed(self.main), Sizing::FILL))
            .max_size(axis.compose_size(f32::INFINITY, max_cross));
        rec.node(id, capped, |rec| {
            let content = axis.compose_sizing(Sizing::fixed(0.0), Sizing::fixed(content_cross));
            rec.leaf(&format!("{id}-content"), Widget::leaf().size(content));
        });
    }
}
