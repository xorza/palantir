use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::layer::Layer;
use crate::layout::axis::Axis;
use crate::layout::types::{
    align::Align,
    align::VAlign,
    sizing::{Sizes, Sizing},
};
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn hstack_arranges_two_buttons_side_by_side() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(800, 600), |ui| {
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
            .node()
    });
    assert_eq!(
        ui.layout[Layer::Main].rect[root.idx()],
        Rect::new(0.0, 0.0, 800.0, 600.0)
    );

    let kids = ui.main_child_rects(root);
    assert_eq!(kids.len(), 2);

    // "Hi" → 16w label + 24 padding + 2*1 stroke = 42w; height = 19.2 + 12 + 2 = 33.2.
    let a = kids[0];
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 42.0);
    assert_eq!(a.size.h, 33.2);

    let b = kids[1];
    assert_eq!(b.min.x, 42.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 33.2);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(200, 300), |ui| {
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
            .node()
    });
    let kids = ui.main_child_rects(root);
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
        let mut ui = Ui::for_test();
        let root = ui.run_at_value(UVec2::new(400, 100), |ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .size((Sizing::fill(case.weights[0]), Sizing::HUG))
                        .show(ui);
                    Frame::new()
                        .id(WidgetId::from_hash("b"))
                        .size((Sizing::fill(case.weights[1]), Sizing::HUG))
                        .show(ui);
                })
                .node()
        });
        let kids = ui.main_child_rects(root);
        assert_eq!(kids[0].size.w, case.widths[0], "{} first", case.label);
        assert_eq!(kids[1].size.w, case.widths[1], "{} second", case.label);
        assert_eq!(kids[1].min.x, case.widths[0], "{} offset", case.label);
    }
}

/// Two equal-weight Fill buttons inside a Fill-width HStack split the
/// 400 px slot evenly at arrange — independent of their label widths.
/// (Was set up against a Hug HStack which hugs to content per WPF
/// semantics; that case is covered by
/// `cross_driver_tests::stretch_semantics::hug_hstack_with_fill_spacer_hugs_to_button`.)
#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(400, 100), |ui| {
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
            .node()
    });
    let kids = ui.main_child_rects(root);
    assert_eq!(kids[0].size.w, 200.0);
    assert_eq!(kids[1].size.w, 200.0);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 200.0);
}

#[test]
fn hstack_justify_distributes_leftover() {
    use crate::layout::types::justify::Justify;
    // 200-wide parent, 40-wide children, no gap.
    // Center: 60 leading. End: 200-40=160. SpaceBetween: 80 between gap.
    // SpaceAround: 30/60/30 pads.
    let cases: &[(&str, Justify, &[f32])] = &[
        ("center", Justify::Center, &[60.0, 100.0]),
        ("end", Justify::End, &[120.0, 160.0]),
        ("space_between", Justify::SpaceBetween, &[0.0, 80.0, 160.0]),
        ("space_around", Justify::SpaceAround, &[30.0, 130.0]),
    ];
    for (label, justify, expected_xs) in cases {
        let mut ui = Ui::for_test();
        let root = ui.run_at_value(UVec2::new(200, 100), |ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .justify(*justify)
                .show(ui, |ui| {
                    for i in 0..expected_xs.len() {
                        Frame::new()
                            .id(WidgetId::from_hash(("c", i)))
                            .size(40.0)
                            .show(ui);
                    }
                })
                .node()
        });
        let kids = ui.main_child_rects(root);
        for (i, want_x) in expected_xs.iter().enumerate() {
            assert_eq!(kids[i].min.x, *want_x, "case: {label} child[{i}].min.x");
        }
    }
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    use crate::layout::types::justify::Justify;
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(200, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .justify(Justify::Center)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size(40.0)
                    .show(ui);
            })
            .node()
    });
    let kids = ui.main_child_rects(root);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 40.0);
    assert_eq!(kids[1].size.w, 120.0);
    assert_eq!(kids[2].min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(400, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size(40.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size(40.0)
                    .show(ui);
            })
            .node()
    });
    let kids = ui.main_child_rects(root);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 50.0);
    assert_eq!(kids[2].min.x, 100.0);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(200, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                    .align(Align::CENTER)
                    .show(ui);
            })
            .node()
    });
    let r = ui.main_child_rects(root)[0];
    // Cross axis 100, child 20 → centered at 40.
    assert_eq!(r.min.y, 40.0);
    assert_eq!(r.size.h, 20.0);
}

#[test]
fn negative_left_margin_spills_outside_slot() {
    // CSS-style negative margin: smaller slot, larger render, shifted negative.
    let mut ui = Ui::for_test();
    let mut button_node = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            button_node = Some(
                Button::new()
                    .id(WidgetId::from_hash("spill"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(30.0)))
                    .margin((-10.0, 0.0, 0.0, 0.0))
                    .show(ui)
                    .node(),
            );
        });
    });
    let r = ui.layout[Layer::Main].rect[button_node.unwrap().idx()];
    assert_eq!(r.min.x, -10.0, "rendered rect spills 10px left of slot");
    assert_eq!(r.min.y, 0.0);
    assert_eq!(
        r.size.w, 50.0,
        "Fixed value is the rendered width, margin doesn't shrink it"
    );
    assert_eq!(r.size.h, 30.0);
}

/// Pass-2 must not double-count non-Fill children in `total_main`. A Hug
/// HStack with a Hug button and a Fill frame in a 200-wide parent hugs
/// to the button's content width (WPF Stretch semantics: the Fill
/// frame contributes its content — zero, here — to the measure, then
/// expands at arrange). Pre-WPF behavior reported 200 (parent's
/// available); a buggy double-count would have reported ~242
/// (button + Fill's measured share).
#[test]
fn hug_hstack_pass2_does_not_double_count_non_fill_children() {
    let mut ui = Ui::for_test();
    let [button_node, root] = ui.run_at_value(UVec2::new(200, 100), |ui| {
        let panel = Panel::hstack().auto_id().show(ui, |ui| {
            let button = Button::new().auto_id().label("Hi").show(ui).node();
            Frame::new()
                .id(WidgetId::from_hash("filler"))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            button
        });
        [panel.inner, panel.node()]
    });
    let button_w = ui.layout_engine.scratch.desired[button_node.idx()].w;
    let root_w = ui.layout_engine.scratch.desired[root.idx()].w;
    // Hug HStack tracks the button's content width — no inflation from
    // the Fill filler, and no double-count (would be > root_w).
    assert_eq!(root_w, button_w);
}

/// Pin: a collapsed child between two active children does not advance
/// the cursor and does not count toward `total_gap`.
#[test]
fn hstack_collapsed_child_neither_advances_cursor_nor_consumes_gap() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value(UVec2::new(200, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .gap(5.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((20.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("hidden"))
                    .size((50.0, 20.0))
                    .collapsed()
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((30.0, 20.0))
                    .show(ui);
            })
            .node()
    });
    let kids = ui.main_child_rects(root);
    let a = kids[0];
    let hidden = kids[1];
    let b = kids[2];

    assert_eq!((a.min.x, a.size.w), (0.0, 20.0));
    assert_eq!((hidden.min.x, hidden.size.w), (20.0, 0.0));
    assert_eq!(hidden.size.h, 0.0);
    assert_eq!((b.min.x, b.size.w), (25.0, 30.0));
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
        let mut ui = Ui::for_test();
        let root = ui.run_at_value(case.viewport, |ui| {
            let panel = match case.axis {
                Axis::X => Panel::hstack(),
                Axis::Y => Panel::vstack(),
            };
            panel
                .auto_id()
                .size(case.axis.compose_size(200.0, 40.0))
                .gap(5.0)
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash((case.label, "fixed")))
                        .size(case.axis.compose_size(20.0, 10.0))
                        .show(ui);

                    let hug_size = match case.axis {
                        Axis::X => Sizes::new(Sizing::HUG, Sizing::fixed(10.0)),
                        Axis::Y => Sizes::new(Sizing::fixed(10.0), Sizing::HUG),
                    };
                    let hug = match case.axis {
                        Axis::X => Panel::hstack(),
                        Axis::Y => Panel::vstack(),
                    };
                    hug.id(WidgetId::from_hash((case.label, "hug")))
                        .size(hug_size)
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash((case.label, "hug-content")))
                                .size(case.axis.compose_size(30.0, 10.0))
                                .show(ui);
                        });

                    let fill_size = match case.axis {
                        Axis::X => Sizes::new(Sizing::FILL, Sizing::fixed(10.0)),
                        Axis::Y => Sizes::new(Sizing::fixed(10.0), Sizing::FILL),
                    };
                    Frame::new()
                        .id(WidgetId::from_hash((case.label, "collapsed-fill")))
                        .size(fill_size)
                        .collapsed()
                        .show(ui);
                    Frame::new()
                        .id(WidgetId::from_hash((case.label, "fill")))
                        .size(fill_size)
                        .show(ui);
                })
                .node()
        });

        let actual = ui.main_child_rects(root);
        let expected = [
            case.axis.compose_rect(0.0, 0.0, 20.0, 10.0),
            case.axis.compose_rect(25.0, 0.0, 30.0, 10.0),
            case.axis.compose_rect(55.0, 0.0, 0.0, 0.0),
            case.axis.compose_rect(60.0, 0.0, 140.0, 10.0),
        ];
        assert_eq!(actual, expected, "case: {}", case.label);
        assert!(
            ui.layout_engine.scratch.stack_fill.pool.is_empty(),
            "case: {} must release its planning scratch",
            case.label,
        );
    }
}

/// Pin: a Fill child's `max_size` caps its arranged width when the
/// freeze loop's share would otherwise exceed the cap. (Measure-time
/// Fill returns content per WPF Stretch; the `max_size` clamp applies
/// in the arrange freeze loop.)
#[test]
fn hstack_fill_max_size_caps_arranged_share() {
    use crate::primitives::size::Size;

    let mut ui = Ui::for_test();
    let mut fill_node = None;
    ui.run_at(UVec2::new(400, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("fixed"))
                    .size((20.0, 20.0))
                    .show(ui);
                fill_node = Some(
                    Frame::new()
                        .id(WidgetId::from_hash("fill"))
                        .size((Sizing::FILL, 20.0))
                        .max_size(Size::new(50.0, f32::INFINITY))
                        .show(ui)
                        .node(),
                );
            });
    });
    let arranged = ui.layout[Layer::Main].rect[fill_node.unwrap().idx()];
    assert_eq!(
        arranged.size.w, 50.0,
        "Fill arrange must clamp to max_size when leftover share > cap"
    );
}

/// Pin: a parent's `max_size` clamps what its children see as
/// `available` during measure. Regression: `measure_dispatch` derived
/// `inner_avail` from raw `available` ignoring `bounds.max_size`.
#[test]
fn parent_max_size_clamps_children_available() {
    use crate::primitives::size::Size;

    let mut ui = Ui::for_test();
    let mut child_node = None;
    let parent_node = ui.under_outer(UVec2::new(1000, 200), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("capped-parent"))
            .size((Sizing::FILL, Sizing::fixed(40.0)))
            .max_size(Size::new(200.0, f32::INFINITY))
            .show(ui, |ui| {
                child_node = Some(
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .size((Sizing::FILL, Sizing::fixed(20.0)))
                        .show(ui, |_| {})
                        .node(),
                );
            })
            .node()
    });
    let parent_rect = ui.layout[Layer::Main].rect[parent_node.idx()];
    assert_eq!(
        parent_rect.size.w, 200.0,
        "parent must arrange at its own max_size cap",
    );
    let inner_rect = ui.layout[Layer::Main].rect[child_node.unwrap().idx()];
    assert_eq!(
        inner_rect.size.w, 200.0,
        "Fill child must not bleed past parent's max_size cap",
    );
}

/// `Sizing::fill` stretches to the parent's cross-axis slot regardless
/// of the child's `align`. Setting `.align(Align::LEFT/CENTER/RIGHT)` on a
/// Fill child used to silently downgrade it to its content size (since
/// cross-axis placement only stretched when `align == Auto && Fill`); now Fill
/// is sufficient on its own. `align` is meaningful only for Hug/Fixed
/// children, which actually have room to be offset inside their slot.
#[test]
fn fill_cross_axis_stretches_regardless_of_align() {
    use crate::Sizing;
    use crate::layout::types::align::Align;

    for align in [Align::LEFT, Align::CENTER, Align::RIGHT] {
        let mut ui = Ui::for_test();
        let mut child = None;
        ui.run_at(UVec2::new(400, 100), |ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(400.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    child = Some(
                        Frame::new()
                            .auto_id()
                            .size((Sizing::FILL, Sizing::fixed(20.0)))
                            .align(align)
                            .show(ui)
                            .node(),
                    );
                });
        });
        let r = ui.layout[Layer::Main].rect[child.unwrap().idx()];
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

/// Cross-cutting min/max contract: a `Hug` panel clamps its
/// content-driven size to `[min_size, max_size]` on each axis — the same
/// `resolve_axis_size` clamp every widget/panel goes through, so this
/// pins the behavior for all of them. Small content floors at `min_size`;
/// large content caps at `max_size`.
#[test]
fn hug_panel_clamps_to_min_and_max_size() {
    // Content 60px tall, `min_size` 100 → floors at 100.
    let mut ui = Ui::for_test();
    let small = ui.run_at_value(UVec2::new(800, 600), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("small"))
            .size((Sizing::HUG, Sizing::HUG))
            .min_size((0.0, 100.0))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(60.0)))
                    .show(ui);
            })
            .node()
    });
    assert_eq!(
        ui.layout[Layer::Main].rect[small.idx()].size.h,
        100.0,
        "Hug floors at min_size when content is smaller",
    );

    // Content 300px tall, `max_size` 120 → caps at 120.
    let mut ui = Ui::for_test();
    let big = ui.run_at_value(UVec2::new(800, 600), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("big"))
            .size((Sizing::HUG, Sizing::HUG))
            .max_size((f32::INFINITY, 120.0))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(300.0)))
                    .show(ui);
            })
            .node()
    });
    assert_eq!(
        ui.layout[Layer::Main].rect[big.idx()].size.h,
        120.0,
        "Hug caps at max_size when content is larger",
    );
}

/// 200×100 hstack with `child_align(VAlign::Center)` and two 40×20
/// children. The first child always inherits the parent default (y=40);
/// the second child either inherits (no override → y=40) or overrides
/// (`VAlign::Bottom` → y=80). Pins both inherit-default propagation
/// and that an override on one child doesn't leak to its sibling.
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
        let mut ui = Ui::for_test();
        let root = ui.run_at_value(UVec2::new(200, 100), |ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::fixed(100.0)))
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                        .show(ui);
                    let mut b = Frame::new()
                        .id(WidgetId::from_hash("b"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(20.0)));
                    if let Some(a) = *second_override {
                        b = b.align(a);
                    }
                    b.show(ui);
                })
                .node()
        });
        let kids = ui.main_child_rects(root);
        let (a, b) = (kids[0], kids[1]);
        assert_eq!(a.min.y, 40.0, "case: {label} a inherits default");
        assert_eq!(a.size.h, 20.0, "case: {label} a.size.h");
        assert_eq!(b.min.y, *second_y, "case: {label} b");
        assert_eq!(b.size.h, 20.0, "case: {label} b.size.h");
    }
}
