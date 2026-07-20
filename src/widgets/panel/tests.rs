use crate::Ui;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::Configure;
use crate::scene::layer::Layer;
use crate::scene::tree::node::NodeId;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// `Surface::apply_to` (called by `Panel::show`) writes the clip bit
/// AND records chrome in `Tree::chrome_table` together. One fixture sweeps every Surface
/// configuration: no surface; paint-only via `From<Background>`;
/// `Surface::scissor`; `Surface::clipped`; `Surface::rounded` with
/// non-zero radius; `Surface::rounded` with zero-radius downgrade.
/// Refactors that touch any mode are caught by the table.
#[test]
fn surface_apply_to_sets_clip_bit_and_chrome() {
    use crate::ClipMode;

    let mut ui = Ui::for_test();
    let mut cases: Vec<(&str, NodeId, ClipMode, bool)> = Vec::new();
    ui.run_at_without_baseline(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let n = Panel::zstack()
                .id(WidgetId::from_hash("none"))
                .size(50.0)
                .show(ui, |_| {})
                .node();
            cases.push(("none", n, ClipMode::None, false));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("paint-only"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                    ..Default::default()
                })
                .show(ui, |_| {})
                .node();
            cases.push(("paint-only", n, ClipMode::None, true));

            // Surface::scissor — clip + transparent paint. Chrome is
            // dropped at install (Tree::open_node filters invisible
            // paint), so only the clip flag survives.
            let n = Panel::zstack()
                .id(WidgetId::from_hash("scissor"))
                .size(50.0)
                .clip_rect()
                .show(ui, |_| {})
                .node();
            cases.push(("scissor", n, ClipMode::Rect, false));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("clipped"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.2, 0.2).into(),
                    ..Default::default()
                })
                .clip_rect()
                .show(ui, |_| {})
                .node();
            cases.push(("clipped", n, ClipMode::Rect, true));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("rounded"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.2, 0.2).into(),
                    corners: Corners::all(4.0),
                    ..Default::default()
                })
                .clip_rounded()
                .show(ui, |_| {})
                .node();
            cases.push(("rounded", n, ClipMode::Rounded, true));

            // Background + clip_rounded with zero radius — Ui::node downgrades.
            let n = Panel::zstack()
                .id(WidgetId::from_hash("rounded-zero"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.2, 0.2).into(),
                    ..Default::default()
                })
                .clip_rounded()
                .show(ui, |_| {})
                .node();
            cases.push(("rounded-zero", n, ClipMode::Rect, true));
        });
    });
    for (name, id, expected_clip, expects_chrome) in &cases {
        let clip = ui.forest.trees[Layer::Main].records.attrs()[id.idx()].clip_mode();
        assert_eq!(clip, *expected_clip, "[{name}] clip mode");
        let chrome = ui.forest.trees[Layer::Main].chrome(*id);
        assert_eq!(
            chrome.is_some(),
            *expects_chrome,
            "[{name}] chrome stamping"
        );
    }
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut ui = Ui::for_test();
    let [panel_node, a_node, b_node] =
        ui.run_at_value_without_baseline(UVec2::new(400, 200), |ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    let panel = Panel::zstack()
                        .id(WidgetId::from_hash("card"))
                        .padding(10.0)
                        .background(Background {
                            fill: Color::rgb(0.1, 0.1, 0.15).into(),
                            corners: Corners::all(8.0),
                            ..Default::default()
                        })
                        .show(ui, |ui| {
                            [
                                Button::new()
                                    .id(WidgetId::from_hash("a"))
                                    .size((Sizing::fixed(80.0), Sizing::fixed(30.0)))
                                    .show(ui)
                                    .node(),
                                Button::new()
                                    .id(WidgetId::from_hash("b"))
                                    .size((Sizing::fixed(60.0), Sizing::fixed(50.0)))
                                    .show(ui)
                                    .node(),
                            ]
                        });
                    [panel.node(), panel.inner[0], panel.inner[1]]
                })
                .inner
        });
    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = ui.layout[Layer::Main].rect[panel_node.idx()];
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    let a = ui.layout[Layer::Main].rect[a_node.idx()];
    let b = ui.layout[Layer::Main].rect[b_node.idx()];
    assert_eq!((a.min.x, a.min.y), (10.0, 10.0));
    assert_eq!((b.min.x, b.min.y), (10.0, 10.0));
    assert_eq!((a.size.w, a.size.h), (80.0, 30.0));
    assert_eq!((b.size.w, b.size.h), (60.0, 50.0));

    assert!(
        ui.forest.trees[Layer::Main]
            .shapes_of(panel_node)
            .next()
            .is_none(),
        "panel chrome doesn't show up in the shape stream"
    );
    assert!(
        ui.forest.trees[Layer::Main].chrome(panel_node).is_some(),
        "panel chrome recorded in chrome table",
    );
}

#[test]
fn panel_with_fill_child_grows_to_panel_inner() {
    let mut ui = Ui::for_test();
    let child_node = ui.run_at_value_without_baseline(UVec2::new(400, 400), |ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("p"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
                    .padding(10.0)
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("filler"))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::rgb(0.5, 0.5, 0.5).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node()
                    })
                    .inner
            })
            .inner
    });
    let child = ui.layout[Layer::Main].rect[child_node.idx()];
    // Panel = 200×100; inner (after padding 10) = 180×80, child fills it at (10, 10).
    assert_eq!(child.min.x, 10.0);
    assert_eq!(child.min.y, 10.0);
    assert_eq!(child.size.w, 180.0);
    assert_eq!(child.size.h, 80.0);
}

/// Regression: a child recorded inside a `.disabled(true)` panel
/// must see `state.disabled = true` *during recording* on its very
/// first frame. Cascade lags by a frame, so without
/// `Forest::ancestor_disabled` first-frame `response_for` returned
/// `disabled=false`, which made the animation cache snap to the
/// alive look on insertion and animate to disabled on frame 2 —
/// visible in the showcase as a flash of "alive" disabled buttons.
#[test]
fn child_inside_disabled_panel_sees_disabled_at_record_time() {
    use crate::primitives::widget_id::WidgetId;
    let mut ui = Ui::for_test();
    let child_id = WidgetId::from_hash("child");
    let observed = ui.run_at_value_without_baseline(UVec2::new(200, 200), |ui| {
        Panel::vstack()
            .auto_id()
            .disabled(true)
            .show(ui, |ui| {
                let observed = ui.response_for(child_id);
                Frame::new().id(child_id).size(10.0).show(ui);
                observed
            })
            .inner
    });
    assert!(
        observed.disabled,
        "child inside disabled panel must see disabled at record time",
    );
}

#[test]
fn disabled_panel_suppresses_clicks_on_descendants() {
    use glam::Vec2;

    let mut ui = Ui::for_test();
    let surface = UVec2::new(400, 200);
    let body = |ui: &mut Ui, captured: Option<&mut bool>| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("locked"))
                .size((Sizing::fixed(200.0), Sizing::fixed(80.0)))
                .padding(20.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.2, 0.2).into(),
                    ..Default::default()
                })
                .disabled(true)
                .show(ui, |ui| {
                    let r = Button::new()
                        .id(WidgetId::from_hash("inside"))
                        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                        .show(ui);
                    if let Some(c) = captured {
                        *c = r.left.clicked();
                    }
                });
        });
    };
    ui.run_at(surface, |ui| body(ui, None));
    ui.click_at(Vec2::new(40.0, 40.0));

    let mut clicked = false;
    ui.run_at_without_baseline(surface, |ui| body(ui, Some(&mut clicked)));
    assert!(!clicked, "button inside disabled panel should not click");
}

#[test]
fn canvas_places_children_at_absolute_positions_and_hugs_bbox() {
    let mut ui = Ui::for_test();
    let [canvas_node, a_node, b_node] =
        ui.run_at_value_without_baseline(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    let canvas = Panel::canvas().id(WidgetId::from_hash("c")).show(ui, |ui| {
                        [
                            Frame::new()
                                .id(WidgetId::from_hash("a"))
                                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                                .position(Vec2::new(10.0, 5.0))
                                .show(ui)
                                .node(),
                            Frame::new()
                                .id(WidgetId::from_hash("b"))
                                .size((Sizing::fixed(30.0), Sizing::fixed(60.0)))
                                .position(Vec2::new(80.0, 40.0))
                                .show(ui)
                                .node(),
                        ]
                    });
                    [canvas.node(), canvas.inner[0], canvas.inner[1]]
                })
                .inner
        });
    let c = ui.layout[Layer::Main].rect[canvas_node.idx()];
    // Hugs bbox: max(10+40, 80+30)=110, max(5+20, 40+60)=100.
    assert_eq!(c.size.w, 110.0);
    assert_eq!(c.size.h, 100.0);

    let a = ui.layout[Layer::Main].rect[a_node.idx()];
    let b = ui.layout[Layer::Main].rect[b_node.idx()];
    assert_eq!((a.min.x, a.min.y), (10.0, 5.0));
    assert_eq!((a.size.w, a.size.h), (40.0, 20.0));
    assert_eq!((b.min.x, b.min.y), (80.0, 40.0));
    assert_eq!((b.size.w, b.size.h), (30.0, 60.0));
}

#[test]
fn zstack_layers_children_without_painting_background() {
    // Wrapped in HStack so the ZStack's Hug-to-children size is honored
    // (root would otherwise expand to surface).
    let mut ui = Ui::for_test();
    let [z, bg_node, fg_node] = ui.run_at_value_without_baseline(UVec2::new(400, 200), |ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let zstack = Panel::zstack()
                    .id(WidgetId::from_hash("layered"))
                    .show(ui, |ui| {
                        [
                            Frame::new()
                                .id(WidgetId::from_hash("bg"))
                                .size((Sizing::fixed(120.0), Sizing::fixed(80.0)))
                                .background(Background {
                                    fill: Color::rgb(0.1, 0.1, 0.2).into(),
                                    ..Default::default()
                                })
                                .show(ui)
                                .node(),
                            Button::new()
                                .id(WidgetId::from_hash("fg"))
                                .size((Sizing::fixed(60.0), Sizing::fixed(30.0)))
                                .show(ui)
                                .node(),
                        ]
                    });
                [zstack.node(), zstack.inner[0], zstack.inner[1]]
            })
            .inner
    });
    assert!(ui.forest.trees[Layer::Main].shapes_of(z).next().is_none());

    let zr = ui.layout[Layer::Main].rect[z.idx()];
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    let bg = ui.layout[Layer::Main].rect[bg_node.idx()];
    let fg = ui.layout[Layer::Main].rect[fg_node.idx()];
    assert_eq!((bg.min.x, bg.min.y), (0.0, 0.0));
    assert_eq!((fg.min.x, fg.min.y), (0.0, 0.0));
    assert_eq!((bg.size.w, bg.size.h), (120.0, 80.0));
    assert_eq!((fg.size.w, fg.size.h), (60.0, 30.0));
}

/// ZStack inner = 200×100, child = 40×20. `align(...)` resolves
/// independently per axis: Center → (100-40)/2 leading; End → inner -
/// child; Start → 0.
#[test]
fn zstack_aligns_child_per_axis() {
    let cases: &[(&str, Align, (f32, f32))] = &[
        ("center", Align::CENTER, (80.0, 40.0)),
        (
            "right_center_independent_axes",
            Align::new(HAlign::Right, VAlign::Center),
            (160.0, 40.0),
        ),
    ];
    for (label, align, expected) in cases {
        let mut ui = Ui::for_test();
        let child_node = ui.run_at_value_without_baseline(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("box"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("c"))
                                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                                .align(*align)
                                .background(Background {
                                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                                    ..Default::default()
                                })
                                .show(ui)
                                .node()
                        })
                        .inner
                })
                .inner
        });
        let r = ui.layout[Layer::Main].rect[child_node.idx()];
        assert_eq!((r.min.x, r.min.y), *expected, "case: {label}");
        assert_eq!(
            (r.size.w, r.size.h),
            (40.0, 20.0),
            "case: {label} Fixed size honored under align"
        );
    }
}
