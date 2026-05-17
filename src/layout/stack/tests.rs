use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::Layer;
use crate::forest::tree::{NodeId};
use crate::layout::types::{align::Align, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

fn child_rects(ui: &Ui, root: NodeId) -> Vec<Rect> {
    ui.forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| ui.layout[Layer::Main].rect[c.id.index()])
        .collect()
}

#[test]
fn hstack_arranges_two_buttons_side_by_side() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(800, 600), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Button::new().auto_id().label("Hi").show(ui);
                    Button::new()
                        .auto_id()
                        .label("World")
                        .size((100.0, Sizing::Hug))
                        .show(ui);
                })
                .node(ui),
        );
    });
    let root = root.unwrap();
    assert_eq!(
        ui.layout[Layer::Main].rect[root.index()],
        Rect::new(0.0, 0.0, 800.0, 600.0)
    );

    let kids = child_rects(&ui, root);
    assert_eq!(kids.len(), 2);

    // "Hi" → 16w label + 24 padding = 40w; height = 19.2 + 12 = 31.2.
    let a = kids[0];
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 40.0);
    assert_eq!(a.size.h, 31.2);

    let b = kids[1];
    assert_eq!(b.min.x, 40.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 31.2);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 300), |ui| {
        root = Some(
            Panel::vstack()
                .auto_id()
                .show(ui, |ui| {
                    Button::new().auto_id().size((Sizing::Hug, 50.0)).show(ui);
                    Button::new()
                        .auto_id()
                        .size((Sizing::Hug, Sizing::FILL))
                        .show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    assert_eq!(kids[0].size.h, 50.0);
    assert_eq!(kids[1].min.y, 50.0);
    assert_eq!(kids[1].size.h, 250.0);
}

#[test]
fn hstack_fill_weights_split_remainder_proportionally() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(400, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("a")
                        .size((Sizing::Fill(1.0), Sizing::Hug))
                        .show(ui);
                    Frame::new()
                        .id_salt("b")
                        .size((Sizing::Fill(3.0), Sizing::Hug))
                        .show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    // 400 leftover / 4 weight = 100 per weight unit → a=100, b=300.
    assert_eq!(kids[0].size.w, 100.0);
    assert_eq!(kids[1].size.w, 300.0);
    assert_eq!(kids[1].min.x, 100.0);
}

#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(400, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Button::new()
                        .id_salt("wide")
                        .label("wide button")
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                    Button::new()
                        .id_salt("narrow")
                        .label("x")
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
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
        let mut root = None;
        ui.run_at(UVec2::new(200, 100), |ui| {
            root = Some(
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::Hug))
                    .justify(*justify)
                    .show(ui, |ui| {
                        for i in 0..expected_xs.len() {
                            Frame::new().id_salt(("c", i)).size(40.0).show(ui);
                        }
                    })
                    .node(ui),
            );
        });
        let kids = child_rects(&ui, root.unwrap());
        for (i, want_x) in expected_xs.iter().enumerate() {
            assert_eq!(kids[i].min.x, *want_x, "case: {label} child[{i}].min.x");
        }
    }
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    use crate::layout::types::justify::Justify;
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .justify(Justify::Center)
                .show(ui, |ui| {
                    Frame::new().id_salt("a").size(40.0).show(ui);
                    Frame::new()
                        .id_salt("filler")
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                    Frame::new().id_salt("c").size(40.0).show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 40.0);
    assert_eq!(kids[1].size.w, 120.0);
    assert_eq!(kids[2].min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(400, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .gap(10.0)
                .show(ui, |ui| {
                    Frame::new().id_salt("a").size(40.0).show(ui);
                    Frame::new().id_salt("b").size(40.0).show(ui);
                    Frame::new().id_salt("c").size(40.0).show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].min.x, 50.0);
    assert_eq!(kids[2].min.x, 100.0);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Fixed(100.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("c")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                        .align(Align::CENTER)
                        .show(ui);
                })
                .node(ui),
        );
    });
    let r = child_rects(&ui, root.unwrap())[0];
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
                    .id_salt("spill")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(30.0)))
                    .margin((-10.0, 0.0, 0.0, 0.0))
                    .show(ui)
                    .node(ui),
            );
        });
    });
    let r = ui.layout[Layer::Main].rect[button_node.unwrap().index()];
    assert_eq!(r.min.x, -10.0, "rendered rect spills 10px left of slot");
    assert_eq!(r.min.y, 0.0);
    assert_eq!(
        r.size.w, 50.0,
        "Fixed value is the rendered width, margin doesn't shrink it"
    );
    assert_eq!(r.size.h, 30.0);
}

/// Pass-2 must not double-count non-Fill children in `total_main`. A Hug
/// HStack with a Hug button and a Fill frame in a 200-wide parent should
/// hug to 200 (button + Fill share), not 216.
#[test]
fn hug_hstack_pass2_does_not_double_count_non_fill_children() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Button::new().auto_id().label("Hi").show(ui);
                    Frame::new()
                        .id_salt("filler")
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                })
                .node(ui),
        );
    });
    assert_eq!(
        ui.layout_engine.scratch.desired[root.unwrap().index()].w,
        200.0
    );
}

/// Pin: a collapsed child between two active children does not advance
/// the cursor and does not count toward `total_gap`.
#[test]
fn hstack_collapsed_child_neither_advances_cursor_nor_consumes_gap() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        root = Some(
            Panel::hstack()
                .auto_id()
                .gap(5.0)
                .show(ui, |ui| {
                    Frame::new().id_salt("a").size((20.0, 20.0)).show(ui);
                    Frame::new()
                        .id_salt("hidden")
                        .size((50.0, 20.0))
                        .collapsed()
                        .show(ui);
                    Frame::new().id_salt("b").size((30.0, 20.0)).show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    let a = kids[0];
    let hidden = kids[1];
    let b = kids[2];

    assert_eq!((a.min.x, a.size.w), (0.0, 20.0));
    assert_eq!((hidden.min.x, hidden.size.w), (20.0, 0.0));
    assert_eq!(hidden.size.h, 0.0);
    assert_eq!((b.min.x, b.size.w), (25.0, 30.0));
}

/// Pin: a Fill child's `max_size` clamps the measure-time main share.
#[test]
fn hstack_fill_max_size_caps_measured_share() {
    use crate::primitives::size::Size;

    let mut ui = Ui::for_test();
    let mut fill_node = None;
    ui.run_at(UVec2::new(400, 100), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
            .show(ui, |ui| {
                Frame::new().id_salt("fixed").size((20.0, 20.0)).show(ui);
                fill_node = Some(
                    Frame::new()
                        .id_salt("fill")
                        .size((Sizing::FILL, 20.0))
                        .max_size(Size::new(50.0, f32::INFINITY))
                        .show(ui)
                        .node(ui),
                );
            });
    });
    let desired = ui.layout_engine.scratch.desired[fill_node.unwrap().index()];
    assert_eq!(
        desired.w, 50.0,
        "Fill measure must clamp to max_size when leftover share > cap"
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
            .id_salt("capped-parent")
            .size((Sizing::FILL, Sizing::Fixed(40.0)))
            .max_size(Size::new(200.0, f32::INFINITY))
            .show(ui, |ui| {
                child_node = Some(
                    Panel::hstack()
                        .id_salt("inner")
                        .size((Sizing::FILL, Sizing::Fixed(20.0)))
                        .show(ui, |_| {})
                        .node(ui),
                );
            })
            .node(ui)
    });
    let parent_rect = ui.layout[Layer::Main].rect[parent_node.index()];
    assert_eq!(
        parent_rect.size.w, 200.0,
        "parent must arrange at its own max_size cap",
    );
    let inner_rect = ui.layout[Layer::Main].rect[child_node.unwrap().index()];
    assert_eq!(
        inner_rect.size.w, 200.0,
        "Fill child must not bleed past parent's max_size cap",
    );
}
