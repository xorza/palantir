use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::UVec2;

mod sharing;

#[test]
fn hstack_arranges_two_buttons_side_by_side() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Button::new().auto_id().label("Hi").show(ui);
                Button::new()
                    .auto_id()
                    .label("World")
                    .size((100.0, Sizing::HUG))
                    .show(ui);
            })
            .response
            .node()
    });
    assert_eq!(
        h.ui.arranged_rect(Layer::Main, root),
        Rect::new(0.0, 0.0, 800.0, 600.0)
    );

    let kids = h.main_child_rects(root);
    assert_eq!(kids.len(), 2);

    // "Hi": 16 label + 24 padding + 2 stroke = 42w; line height round(19.2×64)/64 = 19.203125, so 33.203125.
    let a = kids[0];
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 42.0);
    assert_eq!(a.size.h, 33.203_125);

    let b = kids[1];
    assert_eq!(b.min.x, 42.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 33.203_125);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut h = UiHarness::new(UVec2::new(200, 300));
    let root = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::FILL))
            .show(ui, |ui| {
                Button::new().auto_id().size((Sizing::HUG, 50.0)).show(ui);
                Button::new()
                    .auto_id()
                    .size((Sizing::HUG, Sizing::FILL))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].size.h, 50.0);
    assert_eq!(kids[1].min.y, 50.0);
    assert_eq!(kids[1].size.h, 250.0);
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
        let mut h = UiHarness::new(UVec2::new(400, 100));
        let root = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("a"))
                        .size((Sizing::fill(case.weights[0]), Sizing::HUG))
                        .show(ui);
                    Block::new()
                        .id(WidgetId::from_hash("b"))
                        .size((Sizing::fill(case.weights[1]), Sizing::HUG))
                        .show(ui);
                })
                .response
                .node()
        });
        let kids = h.main_child_rects(root);
        assert_eq!(kids[0].size.w, case.widths[0], "{} first", case.label);
        assert_eq!(kids[1].size.w, case.widths[1], "{} second", case.label);
        assert_eq!(kids[1].min.x, case.widths[0], "{} offset", case.label);
    }
}

#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("wide"))
                    .label("wide button")
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                Button::new()
                    .id(WidgetId::from_hash("narrow"))
                    .label("x")
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].size.w, 200.0);
    assert_eq!(kids[1].size.w, 200.0);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 200.0);
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
        let mut h = UiHarness::new(UVec2::new(200, 100));
        let root = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .justify(*justify)
                .show(ui, |ui| {
                    for i in 0..expected_xs.len() {
                        Block::new()
                            .id(WidgetId::from_hash(("c", i)))
                            .size(40.0)
                            .show(ui);
                    }
                })
                .response
                .node()
        });
        let kids = h.main_child_rects(root);
        for (i, want_x) in expected_xs.iter().enumerate() {
            assert_eq!(kids[i].min.x, *want_x, "case: {label} child[{i}].min.x");
        }
    }
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    let mut h = UiHarness::new(UVec2::new(200, 100));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .justify(Justify::Center)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("c"))
                    .size(40.0)
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 40.0);
    assert_eq!(kids[1].size.w, 120.0);
    assert_eq!(kids[2].min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("b"))
                    .size(40.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("c"))
                    .size(40.0)
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 50.0);
    assert_eq!(kids[2].min.x, 100.0);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut h = UiHarness::new(UVec2::new(200, 100));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                    .align(Align::CENTER)
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.main_child_rects(root)[0];
    assert_eq!(r.min.y, 40.0);
    assert_eq!(r.size.h, 20.0);
}

#[test]
fn negative_left_margin_spills_outside_slot() {
    let mut h = UiHarness::new(UVec2::new(200, 100));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("spill"))
                .size((Sizing::fixed(50.0), Sizing::fixed(30.0)))
                .margin((-10.0, 0.0, 0.0, 0.0))
                .show(ui);
        });
    });
    let r = h.arranged(WidgetId::from_hash("spill"));
    assert_eq!(r.min.x, -10.0, "rendered rect spills 10px left of slot");
    assert_eq!(r.min.y, 0.0);
    assert_eq!(
        r.size.w, 50.0,
        "Fixed value is the rendered width, margin doesn't shrink it"
    );
    assert_eq!(r.size.h, 30.0);
}

/// Pass 2 must not double-count non-Fill children in `total_main` (a double-count gives ~242).
#[test]
fn hug_hstack_pass2_does_not_double_count_non_fill_children() {
    let mut h = UiHarness::new(UVec2::new(200, 100));
    let [button_node, root] = h.frame_value(|ui| {
        let panel = Panel::hstack().auto_id().show(ui, |ui| {
            let button = Button::new().auto_id().label("Hi").show(ui).node();
            Block::new()
                .id(WidgetId::from_hash("filler"))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            button
        });
        [panel.inner, panel.response.node()]
    });
    let desired = h.engines.layout.cache.captured_desired();
    let button_w = desired[button_node.idx()].w;
    let root_w = desired[root.idx()].w;
    // No inflation from the Fill filler: "Hi" at mono's 8 px per char plus 12 px padding and 1 px border per side = 42.
    assert_eq!([button_w, root_w], [42.0, 42.0]);
}

#[test]
fn stack_mixed_sizing_modes_have_exact_axis_symmetric_layout() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        axis: Axis,
        viewport: UVec2,
    }

    for case in [
        Case {
            label: "horizontal",
            axis: Axis::X,
            viewport: UVec2::new(200, 40),
        },
        Case {
            label: "vertical",
            axis: Axis::Y,
            viewport: UVec2::new(40, 200),
        },
    ] {
        let mut h = UiHarness::new(case.viewport);
        let root = h.frame_value(|ui| {
            let panel = Panel::stack(case.axis);
            panel
                .auto_id()
                .size(case.axis.compose_size(200.0, 40.0))
                .gap(5.0)
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash((case.label, "fixed")))
                        .size(case.axis.compose_size(20.0, 10.0))
                        .show(ui);

                    let hug_size = case.axis.compose_sizing(Sizing::HUG, Sizing::fixed(10.0));
                    let hug = Panel::stack(case.axis);
                    hug.id(WidgetId::from_hash((case.label, "hug")))
                        .size(hug_size)
                        .show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash((case.label, "hug-content")))
                                .size(case.axis.compose_size(30.0, 10.0))
                                .show(ui);
                        });

                    let fill_size = case.axis.compose_sizing(Sizing::FILL, Sizing::fixed(10.0));
                    Block::new()
                        .id(WidgetId::from_hash((case.label, "collapsed-fill")))
                        .size(fill_size)
                        .collapsed()
                        .show(ui);
                    Block::new()
                        .id(WidgetId::from_hash((case.label, "fill")))
                        .size(fill_size)
                        .show(ui);
                })
                .response
                .node()
        });

        let actual = h.main_child_rects(root);
        let expected = [
            case.axis.compose_rect(0.0, 0.0, 20.0, 10.0),
            case.axis.compose_rect(25.0, 0.0, 30.0, 10.0),
            case.axis.compose_rect(55.0, 0.0, 0.0, 0.0),
            case.axis.compose_rect(60.0, 0.0, 140.0, 10.0),
        ];
        assert_eq!(actual, expected, "case: {}", case.label);
        assert!(
            h.engines.layout.scratch.stack.is_empty(),
            "case: {} must release its planning scratch",
            case.label,
        );
    }
}

#[test]
fn hstack_fill_max_size_caps_arranged_share() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    h.frame(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("fixed"))
                    .size((20.0, 20.0))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("fill"))
                    .size((Sizing::FILL, 20.0))
                    .max_size(Size::new(50.0, f32::INFINITY))
                    .show(ui);
            });
    });
    let arranged = h.arranged(WidgetId::from_hash("fill"));
    assert_eq!(
        arranged.size.w, 50.0,
        "Fill arrange must clamp to max_size when leftover share > cap"
    );
}

#[test]
fn parent_max_size_clamps_children_available() {
    let mut h = UiHarness::new(UVec2::new(1000, 200));
    let parent_node = h.under_outer(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("capped-parent"))
            .size((Sizing::FILL, Sizing::fixed(40.0)))
            .max_size(Size::new(200.0, f32::INFINITY))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui, |_| {});
            })
            .response
            .node()
    });
    let parent_rect = h.ui.arranged_rect(Layer::Main, parent_node);
    assert_eq!(
        parent_rect.size.w, 200.0,
        "parent must arrange at its own max_size cap",
    );
    let inner_rect = h.arranged(WidgetId::from_hash("inner"));
    assert_eq!(
        inner_rect.size.w, 200.0,
        "Fill child must not bleed past parent's max_size cap",
    );
}

#[test]
fn fill_cross_axis_stretches_regardless_of_align() {
    for align in [Align::LEFT, Align::CENTER, Align::RIGHT] {
        let mut h = UiHarness::new(UVec2::new(400, 100));
        let mut child = None;
        h.frame(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(400.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    child = Some(
                        Block::new()
                            .auto_id()
                            .size((Sizing::FILL, Sizing::fixed(20.0)))
                            .align(align)
                            .show(ui)
                            .node(),
                    );
                });
        });
        let r = h.ui.arranged_rect(Layer::Main, child.unwrap());
        assert_eq!(
            r.size.w, 400.0,
            "Fill child with align={align:?} must still stretch to parent's full width \
             (got {})",
            r.size.w,
        );
        assert_eq!(
            r.min.x, 0.0,
            "Fill child with align={align:?} must sit at parent origin (no leftover offset \
             when fully stretched), got x={}",
            r.min.x,
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
        let mut h = UiHarness::new(UVec2::new(200, 100));
        let root = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::fixed(100.0)))
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("a"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                        .show(ui);
                    let mut b = Block::new()
                        .id(WidgetId::from_hash("b"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(20.0)));
                    if let Some(a) = *second_override {
                        b = b.align(a);
                    }
                    b.show(ui);
                })
                .response
                .node()
        });
        let kids = h.main_child_rects(root);
        let (a, b) = (kids[0], kids[1]);
        assert_eq!(a.min.y, 40.0, "case: {label} a inherits default");
        assert_eq!(a.size.h, 20.0, "case: {label} a.size.h");
        assert_eq!(b.min.y, *second_y, "case: {label} b");
        assert_eq!(b.size.h, 20.0, "case: {label} b.size.h");
    }
}
