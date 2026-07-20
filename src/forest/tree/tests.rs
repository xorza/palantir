use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::forest::element::{Configure, Element};
use crate::forest::layer::Layer;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::Tree;
use crate::forest::tree::node::NodeId;
use crate::forest::tree::recording::{Placement, RecordingScratch};
use crate::layout::types::{justify::Justify, sizing::Sizing};
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::cmd_buffer::Command;
use crate::shape::style::{LineCap, LineJoin};
use crate::shape::{PolylineColors, Shape};
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(200, 200);

#[test]
#[should_panic(expected = "Tree::open_node received a NodeId that doesn't match the next slot")]
fn open_node_rejects_non_next_id() {
    let mut tree = Tree::default();
    tree.open_node(
        &mut RecordingScratch::default(),
        NodeId(1),
        WidgetId::from_hash("wrong-slot"),
        Element::leaf(),
        None,
    );
}

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::for_test();
    let mut button_node = None;
    ui.run_at_acked(SURFACE, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            button_node = Some(Button::new().auto_id().label("X").show(ui).node());
        });
    });

    // Button chrome lives in `chrome_table`, not in shapes — only the
    // label `Text` shape lands here.
    let shapes: Vec<&ShapeRecord> = ui.forest.trees[Layer::Main]
        .shapes_of(button_node.unwrap())
        .collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], ShapeRecord::Text { .. }));
    assert!(
        ui.forest.trees[Layer::Main]
            .chrome(button_node.unwrap())
            .is_some(),
    );
}

/// Pin record-order interleaving: shapes interleaved with child nodes
/// under one parent surface as `shapes.start` values between parent
/// shape indices in the flat buffer; the encoder paints them in that
/// order.
#[test]
fn interleaved_shapes_record_correct_order() {
    fn pos_rect(slot: u16) -> Shape<'static> {
        let s = (slot + 1) as f32 * 10.0;
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, s, s)),
            corners: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = Ui::for_test();
    let p = ui.run_at_value_acked(SURFACE, |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                ui.add_shape(pos_rect(0));
                Frame::new()
                    .id(WidgetId::from_hash("c0"))
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(1));
                Frame::new()
                    .id(WidgetId::from_hash("c1"))
                    .background(Background {
                        fill: Color::rgb(0.0, 0.0, 1.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(2));
            })
            .node()
    });
    let pi = p.idx();
    let p_shapes = ui.forest.trees[Layer::Main].records.shape_span()[pi];
    assert_eq!(p_shapes.len, 3);
    let children: Vec<_> = ui.main_child_ids(p);
    assert_eq!(children.len(), 2);
    let c0_shapes = ui.forest.trees[Layer::Main].records.shape_span()[children[0].idx()];
    let c1_shapes = ui.forest.trees[Layer::Main].records.shape_span()[children[1].idx()];
    assert_eq!(c0_shapes.start, p_shapes.start + 1);
    assert_eq!(c1_shapes.start, p_shapes.start + 2);
    assert_eq!(
        p_shapes.start + p_shapes.len,
        c1_shapes.start + c1_shapes.len + 1
    );
    let sizes: Vec<f32> = ui.forest.trees[Layer::Main]
        .shapes_of(p)
        .map(|s| match s {
            ShapeRecord::RoundedRect {
                local_rect: Some(rect),
                ..
            } => rect.size.w,
            _ => panic!("unexpected shape variant"),
        })
        .collect();
    assert_eq!(sizes, vec![10.0, 20.0, 30.0]);

    let cmds = ui.encode_cmds();
    let draw_rect_count = cmds
        .iter()
        .filter(|command| matches!(command, Command::DrawRect(_)))
        .count();
    assert_eq!(
        draw_rect_count, 5,
        "3 parent shapes interleaved with 2 child chromes",
    );
}

/// Regression: `subtree_shape_count` must stay correct when a parent
/// pushes shapes after its only child closes (slot=N). Mirrors the
/// scrollbar pattern: `Scroll` has a single `Body` child, then pushes
/// bar `sub-rect`s at slot N. Without the fix, `nodes[Body].shapes.len`
/// over-counts the bars and the encoder cursor overshoots.
#[test]
fn parent_post_child_shapes_dont_inflate_child_subtree_count() {
    fn pos_rect() -> Shape<'static> {
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            corners: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = Ui::for_test();
    let mut child_id = None;
    let mut parent_id = None;
    ui.run_at_acked(SURFACE, |ui| {
        parent_id = Some(
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    child_id = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("only-child"))
                            .background(Background {
                                fill: Color::rgb(0.0, 1.0, 0.0).into(),
                                ..Default::default()
                            })
                            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                            .show(ui)
                            .node(),
                    );
                    ui.add_shape(pos_rect());
                    ui.add_shape(pos_rect());
                })
                .node(),
        );
    });
    let parent = parent_id.unwrap().idx();
    let child = child_id.unwrap().idx();

    assert_eq!(
        ui.forest.trees[Layer::Main].records.subtree_end()[parent],
        ui.forest.trees[Layer::Main].records.subtree_end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );
    assert_eq!(
        ui.forest.trees[Layer::Main].records.shape_span()[parent].len,
        2
    );
    assert_eq!(
        ui.forest.trees[Layer::Main].records.shape_span()[child].len,
        0,
        "child's subtree must NOT include parent's slot-N shapes"
    );

    // Encoder walks without panicking (the original symptom).
    let _cmds = ui.encode_cmds();
}

fn record_hash<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut ui = Ui::for_test();
    let target = ui.run_at_value_acked(SURFACE, |ui| f(ui));
    ui.forest.trees[Layer::Main].rollups.node[target.idx()]
}

fn record_cascade_static<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut ui = Ui::for_test();
    let _ = ui.run_at_value_acked(SURFACE, |ui| f(ui));
    ui.forest.trees[Layer::Main].rollups.cascade_static
}

#[test]
fn empty_tree_has_no_hashes() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |_| {});
    // Synthetic viewport root: present even for an empty user record.
    assert_eq!(ui.forest.trees[Layer::Main].records.len(), 1);
    assert_eq!(ui.forest.trees[Layer::Main].rollups.node.len(), 1);
    assert_eq!(ui.forest.trees[Layer::Main].rollups.subtree.len(), 1);
}

#[test]
fn same_authoring_produces_same_hash() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node()
    };
    assert_eq!(record_hash(build), record_hash(build));
}

#[test]
fn polyline_hash_uses_visual_points_and_lowered_colors() {
    fn build(ui: &mut Ui, points: &[Vec2], color: Color) -> NodeId {
        Panel::canvas()
            .id(WidgetId::from_hash("polyline"))
            .show(ui, |ui| {
                ui.add_shape(Shape::Polyline {
                    points,
                    colors: PolylineColors::Single(color),
                    width: 2.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            })
            .node()
    }

    let base_points = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
    let noisy_points = [Vec2::new(EPS * 0.5, -EPS * 0.5), Vec2::new(10.0, 0.0)];
    let color_a = Color::linear_rgb(0.5, 0.25, 0.75);
    let color_b = Color::linear_rgb(0.5001, 0.2501, 0.7501);
    assert_ne!(color_a, color_b);
    assert_eq!(ColorU8::from(color_a), ColorU8::from(color_b));

    let baseline = record_hash(|ui| build(ui, &base_points, color_a));
    assert_eq!(
        baseline,
        record_hash(|ui| build(ui, &noisy_points, color_a)),
    );
    assert_eq!(baseline, record_hash(|ui| build(ui, &base_points, color_b)),);
}

#[test]
fn changing_fill_color_changes_hash() {
    fn build_child(ui: &mut Ui, fill: Color) -> NodeId {
        let mut child = None;
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                child = Some(
                    Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .size(50.0)
                        .background(Background {
                            fill: fill.into(),
                            ..Default::default()
                        })
                        .show(ui)
                        .node(),
                );
            });
        child.unwrap()
    }
    let h1 = record_hash(|ui| build_child(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_hash(|ui| build_child(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_ne!(h1, h2);
    let static_1 = record_cascade_static(|ui| build_child(ui, Color::rgb(0.2, 0.4, 0.8)));
    let static_2 = record_cascade_static(|ui| build_child(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_eq!(
        static_1, static_2,
        "paint-only changes must remain eligible for incremental cascade"
    );
}

#[test]
fn widget_id_only_affects_cascade_static_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("a"))
            .show(ui, |_| {})
            .node()
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("b"))
            .show(ui, |_| {})
            .node()
    });
    assert_eq!(h1, h2);

    let static_1 = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("a"))
            .show(ui, |_| {})
            .node()
    });
    let static_2 = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("b"))
            .show(ui, |_| {})
            .node()
    });
    assert_ne!(
        static_1, static_2,
        "identity changes must rebuild cascade hit IDs and its by-id snapshot",
    );
}

#[test]
fn changing_layout_property_changes_hash() {
    use crate::forest::visibility::Visibility;
    type Build = fn(&mut Ui) -> NodeId;
    let cases: &[(&str, Build, Build)] = &[
        (
            "size",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::fixed(101.0), Sizing::fixed(50.0)))
                    .show(ui, |_| {})
                    .node()
            },
        ),
        (
            "padding",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .padding(8.0)
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .padding(12.0)
                    .show(ui, |_| {})
                    .node()
            },
        ),
        (
            "visibility",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .visibility(Visibility::Visible)
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .visibility(Visibility::Hidden)
                    .show(ui, |_| {})
                    .node()
            },
        ),
        (
            "justify",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .justify(Justify::Start)
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .justify(Justify::Center)
                    .show(ui, |_| {})
                    .node()
            },
        ),
        (
            "focusable",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .focusable(false)
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .focusable(true)
                    .show(ui, |_| {})
                    .node()
            },
        ),
        (
            "disabled",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .disabled(false)
                    .show(ui, |_| {})
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .disabled(true)
                    .show(ui, |_| {})
                    .node()
            },
        ),
    ];
    for (label, a, b) in cases {
        let h1 = record_hash(*a);
        let h2 = record_hash(*b);
        assert_ne!(h1, h2, "case: {label}");
        let static_1 = record_cascade_static(*a);
        let static_2 = record_cascade_static(*b);
        assert_ne!(
            static_1, static_2,
            "cascade-static hash missed layout case: {label}"
        );
    }
}

#[test]
fn changing_text_content_changes_hash() {
    use crate::widgets::text::Text;
    fn build(ui: &mut Ui, label: &'static str) -> NodeId {
        let mut n = None;
        Panel::hstack().auto_id().show(ui, |ui| {
            n = Some(
                Text::new(label)
                    .id(WidgetId::from_hash("t"))
                    .show(ui)
                    .node(),
            );
        });
        n.unwrap()
    }
    let h1 = record_hash(|ui| build(ui, "Hello"));
    let h2 = record_hash(|ui| build(ui, "World"));
    assert_ne!(h1, h2);
}

#[test]
fn child_hash_does_not_affect_parent_hash() {
    fn build(ui: &mut Ui, fill: Color) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size(50.0)
                    .background(Background {
                        fill: fill.into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node()
    }
    let h1 = record_hash(|ui| build(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_hash(|ui| build(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_eq!(h1, h2, "parent hash captures only its own fields");
}

fn record_subtree_hash<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut ui = Ui::for_test();
    let target = ui.run_at_value_acked(SURFACE, |ui| f(ui));
    ui.forest.trees[Layer::Main].rollups.subtree[target.idx()]
}

#[test]
fn subtree_hash_stable_across_frames() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node()
    };
    assert_eq!(record_subtree_hash(build), record_subtree_hash(build));
}

#[test]
fn subtree_hash_changes_when_descendant_changes() {
    fn build(ui: &mut Ui, fill: Color) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: fill.into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node()
    }
    let h1 = record_subtree_hash(|ui| build(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_subtree_hash(|ui| build(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_ne!(h1, h2, "leaf change must invalidate every ancestor");
}

#[test]
fn subtree_hash_changes_on_sibling_reorder() {
    fn build(ui: &mut Ui, swap: bool) -> NodeId {
        let a = |ui: &mut Ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
        };
        let b = |ui: &mut Ui| {
            Frame::new()
                .id(WidgetId::from_hash("b"))
                .size(30.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.1, 0.1).into(),
                    ..Default::default()
                })
                .show(ui);
        };
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                if swap {
                    b(ui);
                    a(ui);
                } else {
                    a(ui);
                    b(ui);
                }
            })
            .node()
    }
    let h_ab = record_subtree_hash(|ui| build(ui, false));
    let h_ba = record_subtree_hash(|ui| build(ui, true));
    assert_ne!(h_ab, h_ba);
}

/// A panel's own `Panel::transform` changing flips both its
/// `node_hash` and its `subtree_hash`. The `node_hash` change is
/// load-bearing: under the new `Panel::transform` contract, a
/// transform applies to the panel's direct shapes, so a self-transform
/// shift moves the node's *own* painted output. `DamageEngine::compute`
/// keys self-paint damage off `node_hash`, so the bit must live there.
#[test]
fn self_transform_change_flips_node_hash() {
    use crate::primitives::transform::TranslateScale;
    use glam::Vec2;
    fn build(ui: &mut Ui, t: TranslateScale) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .transform(t)
            .show(ui, |_| {})
            .node()
    }
    // Both transforms are non-identity — identity is the noop sentinel
    // (`PanelExtras::DEFAULT.transform`) so a panel with only an
    // identity transform set carries no row at all and the test would
    // be measuring the wrong distinction.
    let t_a = TranslateScale::from_translation(Vec2::new(1.0, 0.0));
    let t_b = TranslateScale::from_translation(Vec2::new(10.0, 0.0));
    let h_node_a = record_hash(|ui| build(ui, t_a));
    let h_node_b = record_hash(|ui| build(ui, t_b));
    assert_ne!(h_node_a, h_node_b, "self transform MUST change node hash");
    let h_sub_a = record_subtree_hash(|ui| build(ui, t_a));
    let h_sub_b = record_subtree_hash(|ui| build(ui, t_b));
    assert_ne!(h_sub_a, h_sub_b, "self transform MUST change subtree hash");
    assert_ne!(
        record_cascade_static(|ui| build(ui, t_a)),
        record_cascade_static(|ui| build(ui, t_b)),
        "self transform MUST change cascade-static hash"
    );

    let identity = TranslateScale::IDENTITY;
    let visual_noop = TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5);
    assert_eq!(
        record_hash(|ui| build(ui, identity)),
        record_hash(|ui| build(ui, visual_noop)),
    );
}

/// `LayoutMode::Grid(idx)` carries a frame-local arena slot. Per-node
/// hash must NOT depend on it — only on def contents (rolled in at
/// `NodeExit`). Same grid declared in different positions still hashes
/// the same.
#[test]
fn grid_per_node_hash_independent_of_arena_slot() {
    use crate::layout::types::track::Track;
    use crate::widgets::grid::Grid;

    let cols = [Track::fill(), Track::fill()];
    let rows = [Track::fill()];

    let mut ui1 = Ui::for_test();
    let mut g1 = None;
    ui1.run_at_acked(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                g1 = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("target"))
                        .cols(cols)
                        .rows(rows)
                        .show(ui, |_| {})
                        .node(),
                );
                Grid::new()
                    .id(WidgetId::from_hash("other"))
                    .cols(cols)
                    .rows(rows)
                    .show(ui, |_| {});
            });
    });
    let mut ui2 = Ui::for_test();
    let mut g2 = None;
    ui2.run_at_acked(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("other"))
                    .cols(cols)
                    .rows(rows)
                    .show(ui, |_| {});
                g2 = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("target"))
                        .cols(cols)
                        .rows(rows)
                        .show(ui, |_| {})
                        .node(),
                );
            });
    });
    assert_eq!(
        ui1.forest.trees[Layer::Main].rollups.node[g1.unwrap().idx()],
        ui2.forest.trees[Layer::Main].rollups.node[g2.unwrap().idx()],
    );
}

#[test]
fn subtree_end_rolls_up_during_recording() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value_acked(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(10.0)
                    .show(ui);
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("b"))
                            .size(10.0)
                            .show(ui);
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(10.0)
                            .show(ui);
                    });
                Frame::new()
                    .id(WidgetId::from_hash("d"))
                    .size(10.0)
                    .show(ui);
            })
            .node()
    });
    // Pre-order: 0=viewport 1=root 2=a 3=inner 4=b 5=c 6=d
    assert_eq!(ui.forest.trees[Layer::Main].records.len(), 7);
    let ends = ui.forest.trees[Layer::Main].records.subtree_end();
    assert_eq!(ends[0].end(), 7, "synthetic viewport spans everything");
    assert_eq!(ends[root.idx()].end(), 7, "root");
    assert_eq!(ends[2].end(), 3, "leaf a");
    assert_eq!(ends[3].end(), 6, "inner spans b,c");
    assert_eq!(ends[4].end(), 5, "leaf b");
    assert_eq!(ends[5].end(), 6, "leaf c");
    assert_eq!(ends[6].end(), 7, "leaf d");
}

#[test]
fn subtree_end_handles_deep_nesting() {
    fn nest(ui: &mut Ui, depth: usize) {
        if depth == 0 {
            Frame::new()
                .id(WidgetId::from_hash(("leaf", depth)))
                .size(10.0)
                .show(ui);
            return;
        }
        Panel::vstack()
            .id(WidgetId::from_hash(("nest", depth)))
            .show(ui, |ui| nest(ui, depth - 1));
    }
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| nest(ui, 16));
    let n = ui.forest.trees[Layer::Main].records.len() as u32;
    // Synthetic viewport + 16 nested vstacks + 1 leaf frame.
    assert_eq!(n, 18);
    for i in 0..(n - 1) {
        assert_eq!(
            ui.forest.trees[Layer::Main].records.subtree_end()[i as usize].end(),
            n,
            "every ancestor on the chain points past the leaf",
        );
    }
    assert_eq!(
        ui.forest.trees[Layer::Main].records.subtree_end()[(n - 1) as usize].end(),
        n,
    );
}

/// `subtree_hash` rollup is root-local: synthesizing a second root by
/// recording two top-level subtrees back-to-back yields independent
/// hashes for the second root regardless of the first's content.
#[test]
fn subtree_hash_rollup_root_local_across_two_roots() {
    fn build(ui: &mut Ui, root_a_color: Color) -> u32 {
        Panel::vstack()
            .id(WidgetId::from_hash("root-a"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a-leaf"))
                    .size(50.0)
                    .background(Background {
                        fill: root_a_color.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
        let b_first = ui.forest.trees[Layer::Main].records.len() as u32;
        Panel::vstack()
            .id(WidgetId::from_hash("root-b"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("b-leaf"))
                    .size(30.0)
                    .show(ui);
            });
        b_first
    }
    let mut ui1 = Ui::for_test();
    let mut b_first1 = 0;
    ui1.run_at_acked(SURFACE, |ui| {
        b_first1 = build(ui, Color::rgb(1.0, 0.0, 0.0));
    });
    let h_b1 = ui1.forest.trees[Layer::Main].rollups.subtree[b_first1 as usize];

    let mut ui2 = Ui::for_test();
    let mut b_first2 = 0;
    ui2.run_at_acked(SURFACE, |ui| {
        b_first2 = build(ui, Color::rgb(0.0, 1.0, 0.0));
    });
    let h_b2 = ui2.forest.trees[Layer::Main].rollups.subtree[b_first2 as usize];
    assert_eq!(b_first1, b_first2);
    assert_eq!(h_b1, h_b2, "root B's subtree_hash must not fold root A");
}

#[test]
fn ui_layer_records_popup_into_separate_tree() {
    let mut ui = Ui::for_test();
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf-2"))
                    .size(30.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup, popup_anchor, None, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    });
    let main_tree = &ui.forest.trees[Layer::Main];
    let popup_tree = &ui.forest.trees[Layer::Popup];
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node.idx(), 0);
    assert_eq!(popup_tree.roots[0].first_node.idx(), 0);
    assert!(
        matches!(
            popup_tree.roots[0].placement,
            Placement::Fixed {
                anchor,
                size: None
            } if anchor == popup_anchor
        ),
        "popup root keeps its fixed layer placement",
    );
    assert_eq!(
        main_tree.records.subtree_end()[0].end() as usize,
        main_tree.records.len(),
    );
    assert_eq!(
        popup_tree.records.subtree_end()[0].end() as usize,
        popup_tree.records.len(),
    );
}

/// `Ui::layer`'s optional size cap selects the overlay's `available`.
/// `None` fills from anchor to surface bottom-right. `Some(s)` is
/// anchor-independent and clamped to the surface; the caller owns
/// placement in that mode. Anchor here is (50, 40) on a 400×300
/// surface; remaining viewport from that anchor is (350, 260).
#[test]
fn ui_layer_size_caps_overlay_available() {
    use crate::primitives::size::Size;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = glam::Vec2::new(50.0, 40.0);
    let cases: &[(Option<Size>, Size)] = &[
        // None → anchor-clamped: surface − anchor.
        (None, Size::new(350.0, 260.0)),
        // Some(s) → anchor-independent: cap unchanged when ≤ surface.
        (Some(Size::new(120.0, 80.0)), Size::new(120.0, 80.0)),
        // Some(huge) → clamped to the full surface size, not to
        // `surface − anchor` (the caller picks the position).
        (Some(Size::new(9999.0, 9999.0)), Size::new(400.0, 300.0)),
        // Some(mixed) → each axis clamps independently to surface.
        (Some(Size::new(100.0, 9999.0)), Size::new(100.0, 300.0)),
    ];
    let mut ui = Ui::for_test();
    for (cap, expected) in cases {
        ui.run_at_acked(SURF, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |_| {});
            ui.layer(Layer::Popup, anchor, *cap, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("overlay-root"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |_| {});
            });
        });
        let popup_tree = &ui.forest.trees[Layer::Popup];
        let root = popup_tree.roots[0].first_node.idx();
        let rect = ui.layout[Layer::Popup].rect[root];
        assert_eq!(rect.min, anchor, "cap={cap:?}");
        assert_eq!(rect.size, *expected, "cap={cap:?}");
    }
}

#[test]
fn empty_popup_body_leaves_popup_tree_empty() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("only-main"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("leaf"))
                    .size(20.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup, glam::Vec2::ZERO, None, |_| {});
    });
    assert_eq!(ui.forest.trees[Layer::Main].roots.len(), 1);
    assert!(ui.forest.trees[Layer::Popup].roots.is_empty());
    assert!(ui.forest.trees[Layer::Popup].records.is_empty());
}

#[test]
fn forest_independence_across_recording_orders() {
    let popup_anchor = glam::Vec2::new(10.0, 10.0);
    let record_main = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
            });
    };
    let record_popup = |ui: &mut Ui| {
        ui.layer(Layer::Popup, popup_anchor, None, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    };
    let mut ui_p_first = Ui::for_test();
    ui_p_first.run_at_acked(UVec2::new(400, 400), |ui| {
        record_popup(ui);
        record_main(ui);
    });
    let mut ui_m_first = Ui::for_test();
    ui_m_first.run_at_acked(UVec2::new(400, 400), |ui| {
        record_main(ui);
        record_popup(ui);
    });
    for layer in [Layer::Main, Layer::Popup] {
        assert_eq!(
            ui_p_first.forest.trees[layer].records.len(),
            ui_m_first.forest.trees[layer].records.len(),
            "{layer:?} record count independent of recording order",
        );
    }
}

#[test]
fn mid_recording_popup_with_text_renders_through_encoder() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let popup_anchor = glam::Vec2::new(50.0, 100.0);
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("outer-main"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("trigger"))
                    .label("menu")
                    .show(ui);
                ui.layer(Layer::Popup, popup_anchor, None, |ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-body"))
                        .show(ui, |ui| {
                            Button::new()
                                .id(WidgetId::from_hash("popup-item"))
                                .label("copy")
                                .show(ui);
                        });
                });
            });
    });
    let _cmds = ui.encode_cmds();

    let payloads = ui.record_store.payloads.borrow();
    let bytes = payloads.text_bytes();
    let main_tree = &ui.forest.trees[Layer::Main];
    let popup_tree = &ui.forest.trees[Layer::Popup];

    let outer_span = main_tree.records.shape_span()[0];
    let main_texts: Vec<&str> = main_tree.shapes.records
        [outer_span.start as usize..(outer_span.start + outer_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(text.resolve(&bytes).text),
            _ => None,
        })
        .collect();
    assert_eq!(main_texts, vec!["menu"]);

    let popup_root_span = popup_tree.records.shape_span()[0];
    let popup_texts: Vec<&str> = popup_tree.shapes.records
        [popup_root_span.start as usize..(popup_root_span.start + popup_root_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(text.resolve(&bytes).text),
            _ => None,
        })
        .collect();
    assert_eq!(popup_texts, vec!["copy"]);
}

/// Pins per-tree shape buffer ownership
/// proven by markers pushed at every Main + Popup level — each appears
/// exactly once, in its owning tree, in recording order.
#[test]
fn mid_recording_popup_keeps_trees_independent() {
    fn marker(slot: u8) -> Shape<'static> {
        let w = (slot + 1) as f32;
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, w, w)),
            corners: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        }
    }
    fn marker_w(s: &ShapeRecord) -> u32 {
        match s {
            ShapeRecord::RoundedRect {
                local_rect: Some(r),
                ..
            } => r.size.w as u32,
            _ => panic!("unexpected shape variant"),
        }
    }

    let mut ui = Ui::for_test();
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    let parent = ui.run_at_value_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-parent"))
            .show(ui, |ui| {
                ui.add_shape(marker(0));
                Frame::new()
                    .id(WidgetId::from_hash("mc1"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(1));
                Frame::new()
                    .id(WidgetId::from_hash("mc2"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(2));
                ui.layer(Layer::Popup, popup_anchor, None, |ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-root"))
                        .show(ui, |ui| {
                            ui.add_shape(marker(10));
                            Frame::new()
                                .id(WidgetId::from_hash("popup-leaf"))
                                .size(10.0)
                                .show(ui);
                            ui.add_shape(marker(11));
                            Frame::new()
                                .id(WidgetId::from_hash("popup-leaf-2"))
                                .size(10.0)
                                .show(ui);
                        });
                });
                ui.add_shape(marker(3));
                Frame::new()
                    .id(WidgetId::from_hash("mc3"))
                    .size(20.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("mc4"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(4));
            })
            .node()
    });
    let main_tree = &ui.forest.trees[Layer::Main];
    let popup_tree = &ui.forest.trees[Layer::Popup];

    // Synthetic viewport at NodeId(0); user "main-parent" at NodeId(1).
    assert_eq!(main_tree.records.len(), 6);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node.idx(), 0);
    assert_eq!(main_tree.records.subtree_end()[parent.idx()].end(), 6);

    let kids: Vec<u32> = main_tree.children(parent).map(|c| c.id.0).collect();
    assert_eq!(kids, vec![2, 3, 4, 5]);

    let widths: Vec<u32> = main_tree.shapes.records.iter().map(marker_w).collect();
    assert_eq!(widths, vec![1, 2, 3, 4, 5]);
    let parent_span = main_tree.records.shape_span()[parent.idx()];
    assert_eq!(parent_span.start, 0);
    assert_eq!(parent_span.len, 5);
    for leaf_idx in [2, 3, 4, 5] {
        assert_eq!(main_tree.records.shape_span()[leaf_idx as usize].len, 0);
    }

    assert_eq!(popup_tree.records.len(), 3);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots[0].first_node.idx(), 0);
    assert_eq!(popup_tree.records.subtree_end()[0].end(), 3);

    let popup_widths: Vec<u32> = popup_tree.shapes.records.iter().map(marker_w).collect();
    assert_eq!(popup_widths, vec![11, 12]);
    let popup_root_span = popup_tree.records.shape_span()[0];
    assert_eq!(popup_root_span.start, 0);
    assert_eq!(popup_root_span.len, 2);
    for leaf_idx in [1, 2] {
        assert_eq!(popup_tree.records.shape_span()[leaf_idx as usize].len, 0);
    }
}

/// `.gap(...)` is panel-only → populates `panel.table` only;
/// `.min_size(...)` populates `bounds.table` only. Pin so a future
/// re-merge or setter mis-routing trips here.
#[test]
fn extras_columns_split_by_field_kind() {
    use crate::primitives::size::Size;

    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("panel-with-gap"))
            .gap(8.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("leaf-with-min"))
                    .min_size(Size::new(20.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("plain-leaf"))
                    .size(10.0)
                    .show(ui);
            });
    });
    assert_eq!(ui.forest.trees[Layer::Main].panel_table.len(), 1);
    assert_eq!(ui.forest.trees[Layer::Main].bounds_table.len(), 1);
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut ui = Ui::for_test();
    let root = ui.run_at_value_acked(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(10.0)
                    .show(ui);
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("b"))
                            .size(10.0)
                            .show(ui);
                    });
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size(10.0)
                    .show(ui);
            })
            .node()
    });
    let kids: Vec<u32> = ui.forest.trees[Layer::Main]
        .children(root)
        .map(|c| c.id.0)
        .collect();
    // Synthetic viewport at NodeId(0); user "root" at NodeId(1).
    assert_eq!(kids, vec![2, 3, 5], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = ui.forest.trees[Layer::Main]
        .children(NodeId(3))
        .map(|c| c.id.0)
        .collect();
    assert_eq!(inner_kids, vec![4], "inner's direct child: b");
}

/// `Tree.shapes.hashes` is parallel to `Tree.shapes.records` after
/// `post_record`: one slot per shape, populated by the existing
/// `compute_rollups` walk so we don't pay a second per-shape sweep.
#[test]
fn shape_hashes_column_sized_to_shape_records() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: glam::Vec2::new(0.0, 0.0),
                    b: glam::Vec2::new(10.0, 10.0),
                    width: 1.0,
                    brush: Color::rgb(1.0, 0.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
                ui.add_shape(Shape::Line {
                    a: glam::Vec2::new(10.0, 10.0),
                    b: glam::Vec2::new(20.0, 20.0),
                    width: 1.0,
                    brush: Color::rgb(0.0, 1.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
            });
    });
    let tree = &ui.forest.trees[Layer::Main];
    assert_eq!(
        tree.shapes.hashes.len(),
        tree.shapes.records.len(),
        "shape_hashes column must be parallel to records",
    );
    // Two distinct shapes ⇒ two distinct hashes. (Different endpoints,
    // different fills.)
    assert_ne!(
        tree.shapes.hashes[0], tree.shapes.hashes[1],
        "distinct shapes must produce distinct per-shape hashes",
    );
    // No shape hash should be the zero default — populated for every
    // record, never skipped.
    for (i, h) in tree.shapes.hashes.iter().enumerate() {
        assert_ne!(
            *h,
            ContentHash::default(),
            "shape_hashes[{i}] left at default — compute_rollups missed a record",
        );
    }
}

/// Per-shape hashes are deterministic across identical-authoring
/// frames. The shape buffer's slot for the same n-th shape on the
/// same widget must hash to the same value frame N and frame N+1
/// — that's the invariant the damage diff depends on.
#[test]
fn shape_hash_stable_across_frames() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: glam::Vec2::new(0.0, 0.0),
                    b: glam::Vec2::new(10.0, 10.0),
                    width: 1.0,
                    brush: Color::rgb(1.0, 0.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
            });
    };
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, build);
    let h0 = ui.forest.trees[Layer::Main].shapes.hashes[0];
    ui.run_at_acked(SURFACE, build);
    let h1 = ui.forest.trees[Layer::Main].shapes.hashes[0];
    assert_eq!(
        h0, h1,
        "same shape authoring must hash identically across frames",
    );
}

/// Changing one shape's authoring inputs flips that shape's hash
/// alone — other shapes on the same owner stay stable. This is the
/// per-shape damage diff's key precondition.
#[test]
fn one_shape_change_only_flips_its_own_hash() {
    let build = |b_endpoint: glam::Vec2, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: glam::Vec2::new(0.0, 0.0),
                    b: glam::Vec2::new(10.0, 10.0),
                    width: 1.0,
                    brush: Color::rgb(1.0, 0.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
                ui.add_shape(Shape::Line {
                    a: glam::Vec2::new(5.0, 5.0),
                    b: b_endpoint,
                    width: 1.0,
                    brush: Color::rgb(0.0, 1.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
            });
    };
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| build(glam::Vec2::new(20.0, 20.0), ui));
    let h0_a = ui.forest.trees[Layer::Main].shapes.hashes[0];
    let h0_b = ui.forest.trees[Layer::Main].shapes.hashes[1];
    ui.run_at_acked(SURFACE, |ui| build(glam::Vec2::new(30.0, 30.0), ui));
    let h1_a = ui.forest.trees[Layer::Main].shapes.hashes[0];
    let h1_b = ui.forest.trees[Layer::Main].shapes.hashes[1];
    assert_eq!(h0_a, h1_a, "unchanged shape 0 must keep its hash");
    assert_ne!(h0_b, h1_b, "changed shape 1 must flip its hash");
}
