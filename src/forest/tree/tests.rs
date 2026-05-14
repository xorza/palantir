use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::rollups::NodeHash;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::types::{justify::Justify, sizing::Sizing};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::cmd_buffer::CmdKind;
use crate::shape::Shape;
use crate::support::internals::ResponseNodeExt;
use crate::support::testing::{encode_cmds, run_at_acked, shapes_of, ui_with_text};
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(200, 200);

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::new();
    let mut button_node = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            button_node = Some(Button::new().auto_id().label("X").show(ui).node(ui));
        });
    });

    // Button chrome lives in `chrome_table`, not in shapes — only the
    // label `Text` shape lands here.
    let shapes: Vec<&ShapeRecord> =
        shapes_of(ui.forest.tree(Layer::Main), button_node.unwrap()).collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], ShapeRecord::Text { .. }));
    assert!(
        ui.forest
            .tree(Layer::Main)
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
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = Ui::new();
    let mut p = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        p = Some(
            Panel::vstack()
                .auto_id()
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    ui.add_shape(pos_rect(0));
                    Frame::new()
                        .id_salt("c0")
                        .background(Background {
                            fill: Color::rgb(0.0, 1.0, 0.0).into(),
                            ..Default::default()
                        })
                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                        .show(ui);
                    ui.add_shape(pos_rect(1));
                    Frame::new()
                        .id_salt("c1")
                        .background(Background {
                            fill: Color::rgb(0.0, 0.0, 1.0).into(),
                            ..Default::default()
                        })
                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                        .show(ui);
                    ui.add_shape(pos_rect(2));
                })
                .node(ui),
        );
    });
    let p = p.unwrap();
    let pi = p.index();
    let p_shapes = ui.forest.tree(Layer::Main).records.shape_span()[pi];
    assert_eq!(p_shapes.len, 3);
    let children: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(p)
        .map(|c| c.id)
        .collect();
    assert_eq!(children.len(), 2);
    let c0_shapes = ui.forest.tree(Layer::Main).records.shape_span()[children[0].index()];
    let c1_shapes = ui.forest.tree(Layer::Main).records.shape_span()[children[1].index()];
    assert_eq!(c0_shapes.start, p_shapes.start + 1);
    assert_eq!(c1_shapes.start, p_shapes.start + 2);
    assert_eq!(
        p_shapes.start + p_shapes.len,
        c1_shapes.start + c1_shapes.len + 1
    );
    let sizes: Vec<f32> = shapes_of(ui.forest.tree(Layer::Main), p)
        .map(|s| match s {
            ShapeRecord::RoundedRect {
                local_rect: Some(rect),
                ..
            } => rect.size.w,
            _ => panic!("unexpected shape variant"),
        })
        .collect();
    assert_eq!(sizes, vec![10.0, 20.0, 30.0]);

    let cmds = encode_cmds(&ui);
    let draw_rect_count = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect))
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
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0).into(),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = Ui::new();
    let mut child_id = None;
    let mut parent_id = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        parent_id = Some(
            Panel::vstack()
                .auto_id()
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .show(ui, |ui| {
                    child_id = Some(
                        Frame::new()
                            .id_salt("only-child")
                            .background(Background {
                                fill: Color::rgb(0.0, 1.0, 0.0).into(),
                                ..Default::default()
                            })
                            .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                            .show(ui)
                            .node(ui),
                    );
                    ui.add_shape(pos_rect());
                    ui.add_shape(pos_rect());
                })
                .node(ui),
        );
    });
    let parent = parent_id.unwrap().index();
    let child = child_id.unwrap().index();

    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[parent],
        ui.forest.tree(Layer::Main).records.subtree_end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.shape_span()[parent].len,
        2
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.shape_span()[child].len,
        0,
        "child's subtree must NOT include parent's slot-N shapes"
    );

    // Encoder walks without panicking (the original symptom).
    let _cmds = encode_cmds(&ui);
}

// --- Authoring-hash tests ---------------------------------------------

fn record_hash<F: FnOnce(&mut Ui) -> NodeId>(f: F) -> NodeHash {
    let mut ui = Ui::new();
    let mut target = None;
    let mut f = Some(f);
    run_at_acked(&mut ui, SURFACE, |ui| {
        target = Some((f.take().unwrap())(ui));
    });
    ui.forest.tree(Layer::Main).rollups.node[target.unwrap().index()]
}

#[test]
fn empty_tree_has_no_hashes() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, SURFACE, |_| {});
    // Synthetic viewport root: present even for an empty user record.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 1);
    assert_eq!(ui.forest.tree(Layer::Main).rollups.node.len(), 1);
    assert_eq!(ui.forest.tree(Layer::Main).rollups.subtree.len(), 1);
}

#[test]
fn same_authoring_produces_same_hash() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node(ui)
    };
    assert_eq!(record_hash(build), record_hash(build));
}

#[test]
fn changing_fill_color_changes_hash() {
    fn build_child(ui: &mut Ui, fill: Color) -> NodeId {
        let mut child = None;
        Panel::hstack().id_salt("root").show(ui, |ui| {
            child = Some(
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: fill.into(),
                        ..Default::default()
                    })
                    .show(ui)
                    .node(ui),
            );
        });
        child.unwrap()
    }
    let h1 = record_hash(|ui| build_child(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_hash(|ui| build_child(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_ne!(h1, h2);
}

#[test]
fn widget_id_does_not_affect_hash() {
    let h1 = record_hash(|ui| Panel::hstack().id_salt("a").show(ui, |_| {}).node(ui));
    let h2 = record_hash(|ui| Panel::hstack().id_salt("b").show(ui, |_| {}).node(ui));
    assert_eq!(h1, h2);
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
                    .id_salt("root")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .size((Sizing::Fixed(101.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
        (
            "padding",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .padding(8.0)
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .padding(12.0)
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
        (
            "visibility",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .visibility(Visibility::Visible)
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .visibility(Visibility::Hidden)
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
        (
            "justify",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .justify(Justify::Start)
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .justify(Justify::Center)
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
        (
            "focusable",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .focusable(false)
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .focusable(true)
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
        (
            "disabled",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .disabled(false)
                    .show(ui, |_| {})
                    .node(ui)
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .disabled(true)
                    .show(ui, |_| {})
                    .node(ui)
            },
        ),
    ];
    for (label, a, b) in cases {
        let h1 = record_hash(*a);
        let h2 = record_hash(*b);
        assert_ne!(h1, h2, "case: {label}");
    }
}

#[test]
fn changing_text_content_changes_hash() {
    use crate::widgets::text::Text;
    fn build(ui: &mut Ui, label: &'static str) -> NodeId {
        let mut n = None;
        Panel::hstack().auto_id().show(ui, |ui| {
            n = Some(Text::new(label).id_salt("t").show(ui).node(ui));
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
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("c")
                    .size(50.0)
                    .background(Background {
                        fill: fill.into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node(ui)
    }
    let h1 = record_hash(|ui| build(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_hash(|ui| build(ui, Color::rgb(0.9, 0.4, 0.8)));
    assert_eq!(h1, h2, "parent hash captures only its own fields");
}

// --- Subtree-hash rollup --------------------------------------------

fn record_subtree_hash<F: FnOnce(&mut Ui) -> NodeId>(f: F) -> NodeHash {
    let mut ui = Ui::new();
    let mut target = None;
    let mut f = Some(f);
    run_at_acked(&mut ui, SURFACE, |ui| {
        target = Some((f.take().unwrap())(ui));
    });
    ui.forest.tree(Layer::Main).rollups.subtree[target.unwrap().index()]
}

#[test]
fn subtree_hash_stable_across_frames() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node(ui)
    };
    assert_eq!(record_subtree_hash(build), record_subtree_hash(build));
}

#[test]
fn subtree_hash_changes_when_descendant_changes() {
    fn build(ui: &mut Ui, fill: Color) -> NodeId {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: fill.into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node(ui)
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
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
        };
        let b = |ui: &mut Ui| {
            Frame::new()
                .id_salt("b")
                .size(30.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.1, 0.1).into(),
                    ..Default::default()
                })
                .show(ui);
        };
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                if swap {
                    b(ui);
                    a(ui);
                } else {
                    a(ui);
                    b(ui);
                }
            })
            .node(ui)
    }
    let h_ab = record_subtree_hash(|ui| build(ui, false));
    let h_ba = record_subtree_hash(|ui| build(ui, true));
    assert_ne!(h_ab, h_ba);
}

/// Transform changes fold into `subtree_hash` only — encode cache
/// (subtree-keyed) invalidates while damage rect-diffing handles paint
/// position drift.
#[test]
fn transform_change_affects_subtree_but_not_node_hash() {
    use crate::primitives::transform::TranslateScale;
    use glam::Vec2;
    fn build(ui: &mut Ui, t: TranslateScale) -> NodeId {
        Panel::hstack()
            .id_salt("root")
            .transform(t)
            .show(ui, |_| {})
            .node(ui)
    }
    let h_node_a = record_hash(|ui| build(ui, TranslateScale::IDENTITY));
    let h_node_b =
        record_hash(|ui| build(ui, TranslateScale::from_translation(Vec2::new(10.0, 0.0))));
    assert_eq!(
        h_node_a, h_node_b,
        "transform must NOT change per-node hash"
    );
    let h_sub_a = record_subtree_hash(|ui| build(ui, TranslateScale::IDENTITY));
    let h_sub_b =
        record_subtree_hash(|ui| build(ui, TranslateScale::from_translation(Vec2::new(10.0, 0.0))));
    assert_ne!(h_sub_a, h_sub_b, "transform MUST change subtree hash");
}

/// `LayoutMode::Grid(idx)` carries a frame-local arena slot. Per-node
/// hash must NOT depend on it — only on def contents (rolled in at
/// `NodeExit`). Same grid declared in different positions still hashes
/// the same.
#[test]
fn grid_per_node_hash_independent_of_arena_slot() {
    use crate::layout::types::track::Track;
    use crate::widgets::grid::Grid;
    use std::rc::Rc;

    let cols: Rc<[Track]> = Rc::from([Track::fill(), Track::fill()]);
    let rows: Rc<[Track]> = Rc::from([Track::fill()]);

    let mut ui1 = Ui::new();
    let mut g1 = None;
    run_at_acked(&mut ui1, SURFACE, |ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            g1 = Some(
                Grid::new()
                    .id_salt("target")
                    .cols(cols.clone())
                    .rows(rows.clone())
                    .show(ui, |_| {})
                    .node(ui),
            );
            Grid::new()
                .id_salt("other")
                .cols(cols.clone())
                .rows(rows.clone())
                .show(ui, |_| {});
        });
    });
    let mut ui2 = Ui::new();
    let mut g2 = None;
    run_at_acked(&mut ui2, SURFACE, |ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Grid::new()
                .id_salt("other")
                .cols(cols.clone())
                .rows(rows.clone())
                .show(ui, |_| {});
            g2 = Some(
                Grid::new()
                    .id_salt("target")
                    .cols(cols.clone())
                    .rows(rows.clone())
                    .show(ui, |_| {})
                    .node(ui),
            );
        });
    });
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[g1.unwrap().index()],
        ui2.forest.tree(Layer::Main).rollups.node[g2.unwrap().index()],
    );
}

// --- subtree_end rollup ---------------------------------------------

#[test]
fn subtree_end_rolls_up_during_recording() {
    let mut ui = Ui::new();
    let mut root = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        root = Some(
            Panel::hstack()
                .id_salt("root")
                .show(ui, |ui| {
                    Frame::new().id_salt("a").size(10.0).show(ui);
                    Panel::hstack().id_salt("inner").show(ui, |ui| {
                        Frame::new().id_salt("b").size(10.0).show(ui);
                        Frame::new().id_salt("c").size(10.0).show(ui);
                    });
                    Frame::new().id_salt("d").size(10.0).show(ui);
                })
                .node(ui),
        );
    });
    // Pre-order: 0=viewport 1=root 2=a 3=inner 4=b 5=c 6=d
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 7);
    let ends = ui.forest.tree(Layer::Main).records.subtree_end();
    assert_eq!(ends[0], 7, "synthetic viewport spans everything");
    assert_eq!(ends[root.unwrap().index()], 7, "root");
    assert_eq!(ends[2], 3, "leaf a");
    assert_eq!(ends[3], 6, "inner spans b,c");
    assert_eq!(ends[4], 5, "leaf b");
    assert_eq!(ends[5], 6, "leaf c");
    assert_eq!(ends[6], 7, "leaf d");
}

#[test]
fn subtree_end_handles_deep_nesting() {
    fn nest(ui: &mut Ui, depth: usize) {
        if depth == 0 {
            Frame::new().id_salt(("leaf", depth)).size(10.0).show(ui);
            return;
        }
        Panel::vstack()
            .id_salt(("nest", depth))
            .show(ui, |ui| nest(ui, depth - 1));
    }
    let mut ui = Ui::new();
    run_at_acked(&mut ui, SURFACE, |ui| nest(ui, 16));
    let n = ui.forest.tree(Layer::Main).records.len() as u32;
    // Synthetic viewport + 16 nested vstacks + 1 leaf frame.
    assert_eq!(n, 18);
    for i in 0..(n - 1) {
        assert_eq!(
            ui.forest.tree(Layer::Main).records.subtree_end()[i as usize],
            n,
            "every ancestor on the chain points past the leaf",
        );
    }
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[(n - 1) as usize],
        n,
    );
}

/// `subtree_hash` rollup is root-local: synthesizing a second root by
/// recording two top-level subtrees back-to-back yields independent
/// hashes for the second root regardless of the first's content.
#[test]
fn subtree_hash_rollup_root_local_across_two_roots() {
    fn build(ui: &mut Ui, root_a_color: Color) -> u32 {
        Panel::vstack().id_salt("root-a").show(ui, |ui| {
            Frame::new()
                .id_salt("a-leaf")
                .size(50.0)
                .background(Background {
                    fill: root_a_color.into(),
                    ..Default::default()
                })
                .show(ui);
        });
        let b_first = ui.forest.tree(Layer::Main).records.len() as u32;
        Panel::vstack().id_salt("root-b").show(ui, |ui| {
            Frame::new().id_salt("b-leaf").size(30.0).show(ui);
        });
        b_first
    }
    let mut ui1 = Ui::new();
    let mut b_first1 = 0;
    run_at_acked(&mut ui1, SURFACE, |ui| {
        b_first1 = build(ui, Color::rgb(1.0, 0.0, 0.0));
    });
    let h_b1 = ui1.forest.tree(Layer::Main).rollups.subtree[b_first1 as usize];

    let mut ui2 = Ui::new();
    let mut b_first2 = 0;
    run_at_acked(&mut ui2, SURFACE, |ui| {
        b_first2 = build(ui, Color::rgb(0.0, 1.0, 0.0));
    });
    let h_b2 = ui2.forest.tree(Layer::Main).rollups.subtree[b_first2 as usize];
    assert_eq!(b_first1, b_first2);
    assert_eq!(h_b1, h_b2, "root B's subtree_hash must not fold root A");
}

#[test]
fn ui_layer_records_popup_into_separate_tree() {
    let mut ui = Ui::new();
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    run_at_acked(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::vstack().id_salt("main-root").show(ui, |ui| {
            Frame::new().id_salt("main-leaf").size(50.0).show(ui);
            Frame::new().id_salt("main-leaf-2").size(30.0).show(ui);
        });
        ui.layer(Layer::Popup, popup_anchor, None, |ui| {
            Panel::vstack().id_salt("popup-root").show(ui, |ui| {
                Frame::new().id_salt("popup-leaf").size(20.0).show(ui);
            });
        });
    });
    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node, 0);
    assert_eq!(popup_tree.roots[0].first_node, 0);
    assert_eq!(popup_tree.roots[0].anchor, popup_anchor);
    assert_eq!(popup_tree.roots[0].size, None);
    assert_eq!(
        main_tree.records.subtree_end()[0] as usize,
        main_tree.records.len(),
    );
    assert_eq!(
        popup_tree.records.subtree_end()[0] as usize,
        popup_tree.records.len(),
    );
}

/// `Ui::layer`'s optional size cap rides through to `LayoutEngine::run` and
/// selects the overlay's `available`: `None` fills from anchor to surface
/// bottom-right; smaller cap wins; oversized cap clamps to viewport so
/// it never bleeds past the surface.
#[test]
fn ui_layer_size_caps_overlay_available() {
    use crate::primitives::size::Size;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = glam::Vec2::new(50.0, 40.0);
    // Remaining viewport = (350, 260).
    let cases: &[(Option<Size>, Size)] = &[
        (None, Size::new(350.0, 260.0)),
        (Some(Size::new(120.0, 80.0)), Size::new(120.0, 80.0)),
        (Some(Size::new(9999.0, 9999.0)), Size::new(350.0, 260.0)),
        (Some(Size::new(100.0, 9999.0)), Size::new(100.0, 260.0)),
    ];
    let mut ui = Ui::new();
    for (cap, expected) in cases {
        run_at_acked(&mut ui, SURF, |ui| {
            Panel::vstack()
                .id_salt("main")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |_| {});
            ui.layer(Layer::Popup, anchor, *cap, |ui| {
                Panel::vstack()
                    .id_salt("overlay-root")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |_| {});
            });
        });
        let popup_tree = ui.forest.tree(Layer::Popup);
        let root = popup_tree.roots[0].first_node as usize;
        let rect = ui.layout[Layer::Popup].rect[root];
        assert_eq!(rect.min, anchor, "cap={cap:?}");
        assert_eq!(rect.size, *expected, "cap={cap:?}");
    }
}

#[test]
fn empty_popup_body_leaves_popup_tree_empty() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, SURFACE, |ui| {
        Panel::vstack().id_salt("only-main").show(ui, |ui| {
            Frame::new().id_salt("leaf").size(20.0).show(ui);
        });
        ui.layer(Layer::Popup, glam::Vec2::ZERO, None, |_| {});
    });
    assert_eq!(ui.forest.tree(Layer::Main).roots.len(), 1);
    assert!(ui.forest.tree(Layer::Popup).roots.is_empty());
    assert!(ui.forest.tree(Layer::Popup).records.is_empty());
}

#[test]
fn forest_independence_across_recording_orders() {
    let popup_anchor = glam::Vec2::new(10.0, 10.0);
    let record_main = |ui: &mut Ui| {
        Panel::vstack().id_salt("main-root").show(ui, |ui| {
            Frame::new().id_salt("main-leaf").size(50.0).show(ui);
        });
    };
    let record_popup = |ui: &mut Ui| {
        ui.layer(Layer::Popup, popup_anchor, None, |ui| {
            Panel::vstack().id_salt("popup-root").show(ui, |ui| {
                Frame::new().id_salt("popup-leaf").size(20.0).show(ui);
            });
        });
    };
    let mut ui_p_first = Ui::new();
    run_at_acked(&mut ui_p_first, UVec2::new(400, 400), |ui| {
        record_popup(ui);
        record_main(ui);
    });
    let mut ui_m_first = Ui::new();
    run_at_acked(&mut ui_m_first, UVec2::new(400, 400), |ui| {
        record_main(ui);
        record_popup(ui);
    });
    for layer in [Layer::Main, Layer::Popup] {
        assert_eq!(
            ui_p_first.forest.tree(layer).records.len(),
            ui_m_first.forest.tree(layer).records.len(),
            "{layer:?} record count independent of recording order",
        );
    }
}

#[test]
fn mid_recording_popup_with_text_renders_through_encoder() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let popup_anchor = glam::Vec2::new(50.0, 100.0);
    run_at_acked(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::vstack().id_salt("outer-main").show(ui, |ui| {
            Button::new().id_salt("trigger").label("menu").show(ui);
            ui.layer(Layer::Popup, popup_anchor, None, |ui| {
                Panel::vstack().id_salt("popup-body").show(ui, |ui| {
                    Button::new().id_salt("popup-item").label("copy").show(ui);
                });
            });
        });
    });
    let _cmds = encode_cmds(&ui);

    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);

    let outer_span = main_tree.records.shape_span()[0];
    let main_texts: Vec<&str> = main_tree.shapes.records
        [outer_span.start as usize..(outer_span.start + outer_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(main_texts, vec!["menu"]);

    let popup_root_span = popup_tree.records.shape_span()[0];
    let popup_texts: Vec<&str> = popup_tree.shapes.records
        [popup_root_span.start as usize..(popup_root_span.start + popup_root_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(popup_texts, vec!["copy"]);
}

/// Mirrors `docs/popups.md` step 4: per-tree shape buffer ownership
/// proven by markers pushed at every Main + Popup level — each appears
/// exactly once, in its owning tree, in recording order.
#[test]
fn mid_recording_popup_keeps_trees_independent() {
    fn marker(slot: u8) -> Shape<'static> {
        let w = (slot + 1) as f32;
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, w, w)),
            radius: Corners::default(),
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

    let mut ui = Ui::new();
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    let mut parent = None;
    run_at_acked(&mut ui, UVec2::new(400, 400), |ui| {
        parent = Some(
            Panel::vstack()
                .id_salt("main-parent")
                .show(ui, |ui| {
                    ui.add_shape(marker(0));
                    Frame::new().id_salt("mc1").size(20.0).show(ui);
                    ui.add_shape(marker(1));
                    Frame::new().id_salt("mc2").size(20.0).show(ui);
                    ui.add_shape(marker(2));
                    ui.layer(Layer::Popup, popup_anchor, None, |ui| {
                        Panel::vstack().id_salt("popup-root").show(ui, |ui| {
                            ui.add_shape(marker(10));
                            Frame::new().id_salt("popup-leaf").size(10.0).show(ui);
                            ui.add_shape(marker(11));
                            Frame::new().id_salt("popup-leaf-2").size(10.0).show(ui);
                        });
                    });
                    ui.add_shape(marker(3));
                    Frame::new().id_salt("mc3").size(20.0).show(ui);
                    Frame::new().id_salt("mc4").size(20.0).show(ui);
                    ui.add_shape(marker(4));
                })
                .node(ui),
        );
    });
    let parent = parent.unwrap();
    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);

    // Synthetic viewport at NodeId(0); user "main-parent" at NodeId(1).
    assert_eq!(main_tree.records.len(), 6);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node, 0);
    assert_eq!(main_tree.records.subtree_end()[parent.index()], 6);

    let kids: Vec<u32> = main_tree.children(parent).map(|c| c.id.0).collect();
    assert_eq!(kids, vec![2, 3, 4, 5]);

    let widths: Vec<u32> = main_tree.shapes.records.iter().map(marker_w).collect();
    assert_eq!(widths, vec![1, 2, 3, 4, 5]);
    let parent_span = main_tree.records.shape_span()[parent.index()];
    assert_eq!(parent_span.start, 0);
    assert_eq!(parent_span.len, 5);
    for leaf_idx in [2, 3, 4, 5] {
        assert_eq!(main_tree.records.shape_span()[leaf_idx as usize].len, 0);
    }

    assert_eq!(popup_tree.records.len(), 3);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots[0].first_node, 0);
    assert_eq!(popup_tree.records.subtree_end()[0], 3);

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

    let mut ui = Ui::new();
    run_at_acked(&mut ui, SURFACE, |ui| {
        Panel::hstack()
            .id_salt("panel-with-gap")
            .gap(8.0)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("leaf-with-min")
                    .min_size(Size::new(20.0, 20.0))
                    .show(ui);
                Frame::new().id_salt("plain-leaf").size(10.0).show(ui);
            });
    });
    assert_eq!(ui.forest.tree(Layer::Main).panel_table.len(), 1);
    assert_eq!(ui.forest.tree(Layer::Main).bounds_table.len(), 1);
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut ui = Ui::new();
    let mut root = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        root = Some(
            Panel::hstack()
                .id_salt("root")
                .show(ui, |ui| {
                    Frame::new().id_salt("a").size(10.0).show(ui);
                    Panel::hstack().id_salt("inner").show(ui, |ui| {
                        Frame::new().id_salt("b").size(10.0).show(ui);
                    });
                    Frame::new().id_salt("c").size(10.0).show(ui);
                })
                .node(ui),
        );
    });
    let kids: Vec<u32> = ui
        .forest
        .tree(Layer::Main)
        .children(root.unwrap())
        .map(|c| c.id.0)
        .collect();
    // Synthetic viewport at NodeId(0); user "root" at NodeId(1).
    assert_eq!(kids, vec![2, 3, 5], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = ui
        .forest
        .tree(Layer::Main)
        .children(NodeId(3))
        .map(|c| c.id.0)
        .collect();
    assert_eq!(inner_kids, vec![4], "inner's direct child: b");
}
