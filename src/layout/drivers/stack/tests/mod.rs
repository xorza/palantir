use crate::internals::layout_rig::{LayoutRig, NodeRecorder, TextRun};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::axis::{Axis, AxisExtent};
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

mod sharing;

/// A `Hug` leaf holding one line of `text` in the 16 px mono face: 8 px a character, 16 tall.
fn label(rec: &mut NodeRecorder<'_>, id: &str, leaf: Widget, text: &str) {
    rec.text_leaf(id, leaf, TextRun::new(text, 16.0, TextWrap::SingleLine));
}

#[test]
fn hstack_arranges_two_labels_side_by_side() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        let fill = Widget::hstack().size((Sizing::FILL, Sizing::FILL));
        rec.node("root", fill, |rec| {
            label(rec, "hi", Widget::leaf(), "Hi");
            label(
                rec,
                "world",
                Widget::leaf().size((100.0, Sizing::HUG)),
                "World",
            );
        });
    });
    assert_eq!(rig.rect("root"), Rect::new(0.0, 0.0, 800.0, 600.0));
    // "Hi": 2 × 8 = 16 wide, one 16 px line.
    assert_eq!(rig.rect("hi"), Rect::new(0.0, 0.0, 16.0, 16.0));
    assert_eq!(rig.rect("world"), Rect::new(16.0, 0.0, 100.0, 16.0));
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 300.0), |rec| {
        let column = Widget::vstack().size((Sizing::HUG, Sizing::FILL));
        rec.node("root", column, |rec| {
            rec.leaf("a", Widget::leaf().size((Sizing::HUG, 50.0)));
            rec.leaf("b", Widget::leaf().size((Sizing::HUG, Sizing::FILL)));
        });
    });
    assert_eq!(rig.rect("a").size.h, 50.0);
    assert_eq!((rig.rect("b").min.y, rig.rect("b").size.h), (50.0, 250.0));
}

#[test]
fn hstack_fill_weights_split_remainder_proportionally() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        weights: [f32; 2],
        widths: [f32; 2],
    }

    for case in [
        Case {
            label: "one_to_three",
            weights: [1.0, 3.0],
            widths: [100.0, 300.0],
        },
        Case {
            label: "maximum_finite_weights",
            weights: [f32::MAX, f32::MAX],
            widths: [200.0, 200.0],
        },
    ] {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(400.0, 100.0), |rec| {
            rec.node(
                "root",
                Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
                |rec| {
                    for (id, weight) in ["a", "b"].into_iter().zip(case.weights) {
                        rec.leaf(id, Widget::leaf().size((Sizing::fill(weight), Sizing::HUG)));
                    }
                },
            );
        });
        let kids = rig.child_rects("root");
        assert_eq!(kids[0].size.w, case.widths[0], "{} first", case.label);
        assert_eq!(kids[1].size.w, case.widths[1], "{} second", case.label);
        assert_eq!(kids[1].min.x, case.widths[0], "{} offset", case.label);
    }
}

#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        rec.node(
            "root",
            Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
            |rec| {
                let fill = || Widget::leaf().size((Sizing::FILL, Sizing::HUG));
                label(rec, "wide", fill(), "wide label");
                label(rec, "narrow", fill(), "x");
            },
        );
    });
    assert_eq!(rig.rect("wide"), Rect::new(0.0, 0.0, 200.0, 16.0));
    assert_eq!(rig.rect("narrow"), Rect::new(200.0, 0.0, 200.0, 16.0));
}

#[test]
fn hstack_justify_distributes_leftover() {
    // 200-wide parent, 40-wide children. Two leave 120: Center leads 60, End starts the last at 160,
    // SpaceAround pads 30/60/30. Three leave 80: SpaceBetween makes two 40 px gaps.
    let cases: &[(&str, Justify, &[f32])] = &[
        ("center", Justify::Center, &[60.0, 100.0]),
        ("end", Justify::End, &[120.0, 160.0]),
        ("space_between", Justify::SpaceBetween, &[0.0, 80.0, 160.0]),
        ("space_around", Justify::SpaceAround, &[30.0, 130.0]),
    ];
    for (label, justify, expected_xs) in cases {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(200.0, 100.0), |rec| {
            let row = Widget::hstack()
                .size((Sizing::FILL, Sizing::HUG))
                .justify(*justify);
            rec.node("root", row, |rec| {
                for i in 0..expected_xs.len() {
                    rec.leaf(&format!("c{i}"), Widget::leaf().size(40.0));
                }
            });
        });
        let kids = rig.child_rects("root");
        for (i, want_x) in expected_xs.iter().enumerate() {
            assert_eq!(kids[i].min.x, *want_x, "case: {label} child[{i}].min.x");
        }
    }
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 100.0), |rec| {
        let row = Widget::hstack()
            .size((Sizing::FILL, Sizing::HUG))
            .justify(Justify::Center);
        rec.node("root", row, |rec| {
            rec.leaf("a", Widget::leaf().size(40.0));
            rec.leaf("filler", Widget::leaf().size((Sizing::FILL, Sizing::HUG)));
            rec.leaf("c", Widget::leaf().size(40.0));
        });
    });
    assert_eq!(rig.rect("a").min.x, 0.0);
    assert_eq!(
        (rig.rect("filler").min.x, rig.rect("filler").size.w),
        (40.0, 120.0)
    );
    assert_eq!(rig.rect("c").min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        rec.node("root", Widget::hstack().gap(10.0), |rec| {
            for id in ["a", "b", "c"] {
                rec.leaf(id, Widget::leaf().size(40.0));
            }
        });
    });
    let xs = rig
        .child_rects("root")
        .iter()
        .map(|r| r.min.x)
        .collect::<Vec<_>>();
    assert_eq!(xs, [0.0, 50.0, 100.0]);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 100.0), |rec| {
        let row = Widget::hstack().size((Sizing::FILL, Sizing::fixed(100.0)));
        rec.node("root", row, |rec| {
            rec.leaf("c", Widget::leaf().size((40.0, 20.0)).align(Align::CENTER));
        });
    });
    let r = rig.rect("c");
    assert_eq!((r.min.y, r.size.h), (40.0, 20.0));
}

#[test]
fn negative_left_margin_spills_outside_slot() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 100.0), |rec| {
        rec.node("root", Widget::hstack(), |rec| {
            rec.leaf(
                "spill",
                Widget::leaf()
                    .size((50.0, 30.0))
                    .margin((-10.0, 0.0, 0.0, 0.0)),
            );
        });
    });
    assert_eq!(
        rig.rect("spill"),
        Rect::new(-10.0, 0.0, 50.0, 30.0),
        "the rect spills 10 px left of its slot, and the margin does not shrink the Fixed width",
    );
}

/// A Fill child's share must not count the non-Fill children twice: the Hug row measures what its
/// label wants, 2 × 8 = 16, and no more, though a Fill sibling follows it.
#[test]
fn hug_hstack_pass2_does_not_double_count_non_fill_children() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 100.0), |rec| {
        rec.node("root", Widget::hstack(), |rec| {
            label(rec, "hi", Widget::leaf(), "Hi");
            rec.leaf("filler", Widget::leaf().size((Sizing::FILL, Sizing::HUG)));
        });
    });
    assert_eq!([rig.desired("hi").w, rig.desired("root").w], [16.0, 16.0]);
}

#[test]
fn stack_mixed_sizing_modes_have_exact_axis_symmetric_layout() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        axis: Axis,
        viewport: Size,
    }

    for case in [
        Case {
            label: "horizontal",
            axis: Axis::X,
            viewport: Size::new(200.0, 40.0),
        },
        Case {
            label: "vertical",
            axis: Axis::Y,
            viewport: Size::new(40.0, 200.0),
        },
    ] {
        let axis = case.axis;
        let mut rig = LayoutRig::new();
        rig.run(case.viewport, |rec| {
            let stack = Widget::stack(axis)
                .size(axis.compose_size(200.0, 40.0))
                .gap(5.0);
            rec.node("root", stack, |rec| {
                rec.leaf("fixed", Widget::leaf().size(axis.compose_size(20.0, 10.0)));
                let hug_size = axis.compose_sizing(Sizing::HUG, Sizing::fixed(10.0));
                rec.node("hug", Widget::stack(axis).size(hug_size), |rec| {
                    rec.leaf(
                        "hug-content",
                        Widget::leaf().size(axis.compose_size(30.0, 10.0)),
                    );
                });
                let fill_size = axis.compose_sizing(Sizing::FILL, Sizing::fixed(10.0));
                rec.leaf("collapsed-fill", Widget::leaf().size(fill_size).collapsed());
                rec.leaf("fill", Widget::leaf().size(fill_size));
            });
        });

        let rect = |main_start, main_len, cross_len| {
            axis.compose_rect(
                AxisExtent::new(main_start, main_len),
                AxisExtent::new(0.0, cross_len),
            )
        };
        let expected = [
            rect(0.0, 20.0, 10.0),
            rect(25.0, 30.0, 10.0),
            rect(55.0, 0.0, 0.0),
            rect(60.0, 140.0, 10.0),
        ];
        assert_eq!(rig.child_rects("root"), expected, "case: {}", case.label);
        assert!(
            rig.engine().scratch.stack.is_empty(),
            "case: {} must release its planning scratch",
            case.label,
        );
    }
}

#[test]
fn hstack_fill_max_size_caps_arranged_share() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        let row = Widget::hstack().size((Sizing::fixed(200.0), Sizing::fixed(40.0)));
        rec.node("root", row, |rec| {
            rec.leaf("fixed", Widget::leaf().size((20.0, 20.0)));
            rec.leaf(
                "fill",
                Widget::leaf()
                    .size((Sizing::FILL, 20.0))
                    .max_size(Size::new(50.0, f32::INFINITY)),
            );
        });
    });
    assert_eq!(
        rig.rect("fill").size.w,
        50.0,
        "Fill arrange must clamp to max_size when leftover share > cap"
    );
}

#[test]
fn parent_max_size_clamps_children_available() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(1000.0, 200.0), |rec| {
        let capped = Widget::vstack()
            .size((Sizing::FILL, Sizing::fixed(40.0)))
            .max_size(Size::new(200.0, f32::INFINITY));
        rec.node("capped-parent", capped, |rec| {
            rec.leaf(
                "inner",
                Widget::hstack().size((Sizing::FILL, Sizing::fixed(20.0))),
            );
        });
    });
    assert_eq!(
        rig.rect("capped-parent").size.w,
        200.0,
        "parent must arrange at its own max_size cap",
    );
    assert_eq!(
        rig.rect("inner").size.w,
        200.0,
        "Fill child must not bleed past parent's max_size cap",
    );
}

#[test]
fn fill_cross_axis_stretches_regardless_of_align() {
    for align in [Align::LEFT, Align::CENTER, Align::RIGHT] {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(400.0, 100.0), |rec| {
            let column = Widget::vstack().size((Sizing::fixed(400.0), Sizing::fixed(100.0)));
            rec.node("root", column, |rec| {
                rec.leaf(
                    "child",
                    Widget::leaf()
                        .size((Sizing::FILL, Sizing::fixed(20.0)))
                        .align(align),
                );
            });
        });
        let r = rig.rect("child");
        assert_eq!(
            (r.min.x, r.size.w),
            (0.0, 400.0),
            "a Fill child with align={align:?} must stretch to the parent's full width from its \
             origin",
        );
    }
}

/// A child's `align` override must not leak to its sibling, which inherits `child_align`.
#[test]
fn hstack_child_align_per_axis_with_overrides() {
    let cases: &[(&str, Option<Align>, f32)] = &[
        ("both_inherit_parent_center", None, 40.0),
        (
            "second_overrides_to_bottom",
            Some(Align::v(VAlign::Bottom)),
            80.0,
        ),
    ];
    for (label, second_override, second_y) in cases {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(200.0, 100.0), |rec| {
            let row = Widget::hstack()
                .size((Sizing::FILL, Sizing::fixed(100.0)))
                .child_align(Align::v(VAlign::Center));
            rec.node("root", row, |rec| {
                rec.leaf("a", Widget::leaf().size((40.0, 20.0)));
                let mut b = Widget::leaf().size((40.0, 20.0));
                if let Some(a) = *second_override {
                    b = b.align(a);
                }
                rec.leaf("b", b);
            });
        });
        let (a, b) = (rig.rect("a"), rig.rect("b"));
        assert_eq!(
            (a.min.y, a.size.h),
            (40.0, 20.0),
            "case: {label} a inherits default"
        );
        assert_eq!((b.min.y, b.size.h), (*second_y, 20.0), "case: {label} b");
    }
}
