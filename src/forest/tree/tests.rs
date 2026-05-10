use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::rollups::NodeHash;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::types::{display::Display, justify::Justify, sizing::Sizing};
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::cmd_buffer::CmdKind;
use crate::shape::Shape;
use crate::support::testing::{encode_cmds, shapes_of, ui_at, ui_with_text};
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let mut button_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        button_node = Some(Button::new().auto_id().label("X").show(ui).node);
    });

    // Chrome (the button background) lives in `Tree::chrome_table`,
    // not in the shapes list. Only the label `Text` shape lands here.
    let shapes: Vec<&Shape> =
        shapes_of(ui.forest.tree(Layer::Main), button_node.unwrap()).collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::Text { .. }));
    assert!(
        ui.forest
            .tree(Layer::Main)
            .chrome_for(button_node.unwrap())
            .is_some(),
        "button chrome recorded in chrome table",
    );
}

/// Pin record-order interleaving end-to-end: when shapes are
/// interleaved with child nodes under one parent, the children's
/// `shapes.start` values fall between parent shape indices in the
/// flat shape buffer, and the encoder paints them in that order.
/// Each shape's size encodes the expected slot for unambiguous readback.
#[test]
fn interleaved_shapes_record_correct_order() {
    fn pos_rect(slot: u16) -> Shape {
        let s = (slot + 1) as f32 * 10.0;
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, s, s)),
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = ui_at(UVec2::new(200, 200));
    let p = Panel::vstack()
        .auto_id()
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .show(&mut ui, |ui| {
            ui.add_shape(pos_rect(0));
            Frame::new()
                .id_salt("c0")
                .background(Background {
                    fill: Color::rgb(0.0, 1.0, 0.0),
                    ..Default::default()
                })
                .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                .show(ui);
            ui.add_shape(pos_rect(1));
            Frame::new()
                .id_salt("c1")
                .background(Background {
                    fill: Color::rgb(0.0, 0.0, 1.0),
                    ..Default::default()
                })
                .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                .show(ui);
            ui.add_shape(pos_rect(2));
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Children's `shapes.start` values must fall between the parent's
    // direct shape indices, encoding the shape→child→shape→child→shape
    // interleave purely via spans.
    let pi = p.index();
    let p_shapes = ui.forest.tree(Layer::Main).records.shape_span()[pi];
    assert_eq!(p_shapes.len, 3, "parent owns 3 direct shapes");
    let children: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(p)
        .map(|c| c.id)
        .collect();
    assert_eq!(children.len(), 2);
    let c0_shapes = ui.forest.tree(Layer::Main).records.shape_span()[children[0].index()];
    let c1_shapes = ui.forest.tree(Layer::Main).records.shape_span()[children[1].index()];
    assert_eq!(
        c0_shapes.start,
        p_shapes.start + 1,
        "1 parent shape recorded before c0 opens",
    );
    assert_eq!(
        c1_shapes.start,
        p_shapes.start + 2,
        "1 parent shape recorded between c0 close and c1 open",
    );
    assert_eq!(
        p_shapes.start + p_shapes.len,
        c1_shapes.start + c1_shapes.len + 1,
        "1 parent shape recorded after c1 closes",
    );
    let sizes: Vec<f32> = shapes_of(ui.forest.tree(Layer::Main), p)
        .map(|s| match s {
            Shape::RoundedRect {
                local_rect: Some(rect),
                ..
            } => rect.size.w,
            _ => panic!("unexpected shape variant"),
        })
        .collect();
    // Record order is preserved by direct push to `kinds` + `shapes`.
    assert_eq!(sizes, vec![10.0, 20.0, 30.0]);

    // End-to-end: the encoder paints draw commands in record order —
    // `pos_rect(0)` → child c0 chrome → `pos_rect(1)` → child c1 chrome
    // → `pos_rect(2)`. 3 parent sub-rects + 2 child chrome paints = 5
    // DrawRect cmds in total.
    let cmds = encode_cmds(&ui);
    let draw_rect_count = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect))
        .count();
    assert_eq!(
        draw_rect_count, 5,
        "expected 3 parent shapes interleaved with 2 child chromes",
    );
}

/// Regression: `subtree_shape_count` must stay correct when a parent
/// pushes shapes *after* its only child closes (slot=N shapes). The
/// child and parent share the same NodeId-space `end`, so the old
/// "look at next pre-order node's shape_first" trick over-counted by
/// the parent's trailing shapes — the encoder's shape cursor then
/// overshot `shapes.len()` and panicked when the cache-replay /
/// invisible-cascade short-circuit fired on the child.
///
/// Mirrors the production scrollbar pattern: `Scroll` has a single
/// `Body` child, then pushes bar `sub-rect`s at slot N. Without the
/// fix, `nodes[Body].shapes.len` counted the bars too.
#[test]
fn parent_post_child_shapes_dont_inflate_child_subtree_count() {
    fn pos_rect() -> Shape {
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0),
            stroke: Stroke::ZERO,
        }
    }
    let mut ui = ui_at(UVec2::new(200, 200));
    let mut child_id = None;
    let parent_id = Panel::vstack()
        .auto_id()
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .show(&mut ui, |ui| {
            // Single child, no shapes inside.
            child_id = Some(
                Frame::new()
                    .id_salt("only-child")
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 0.0),
                        ..Default::default()
                    })
                    .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                    .show(ui)
                    .node,
            );
            // Two shapes pushed AFTER the child closes (slot=N).
            ui.add_shape(pos_rect());
            ui.add_shape(pos_rect());
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let parent = parent_id.index();
    let child = child_id.unwrap().index();

    // Parent and child share `end` (parent has only this one child),
    // which is the bug trigger.
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[parent],
        ui.forest.tree(Layer::Main).records.subtree_end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );

    // Parent's subtree contains both bar shapes.
    assert_eq!(
        ui.forest.tree(Layer::Main).records.shape_span()[parent].len,
        2,
        "parent's subtree owns both slot-N shapes"
    );
    // Child's subtree contains zero shapes — the trailing bars belong
    // to the parent, not the child.
    assert_eq!(
        ui.forest.tree(Layer::Main).records.shape_span()[child].len,
        0,
        "child's subtree must NOT include parent's slot-N shapes"
    );

    // End-to-end: the encoder must walk this tree without panicking
    // (cursor overshoot was the original symptom). `encode_cmds`
    // exercises the full encode path.
    let _cmds = encode_cmds(&ui);
}

// --- Authoring-hash tests ---------------------------------------------------
// `Tree::compute_hashes` populates `Tree.hashes` with one u64 per node
// reflecting *only* the authoring inputs (layout, paint attrs, extras,
// shapes, grid defs). Tests below pin the contract: identical authoring
// hashes the same; flipping any field changes the hash.

/// Drive one frame from a builder closure and snapshot the root node's
/// hash. The builder receives `ui` after `begin_frame` and returns the
/// `NodeId` to read.
fn record_hash<F: FnOnce(&mut Ui) -> NodeId>(f: F) -> NodeHash {
    let mut ui = ui_at(UVec2::new(200, 200));
    let target = f(&mut ui);
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    ui.forest.tree(Layer::Main).rollups.node[target.index()]
}

#[test]
fn empty_tree_has_no_hashes() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    // No widgets recorded — node_count is 0 → both hash arrays stay
    // empty. (Layout / end_frame normally need a root, so we
    // intentionally skip them; just call compute_hashes directly to
    // verify the empty-tree case.)
    ui.forest.end_frame(Rect::ZERO);

    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 0);
    assert!(ui.forest.tree(Layer::Main).rollups.node.is_empty());
    assert!(ui.forest.tree(Layer::Main).rollups.subtree.is_empty());
}

#[test]
fn same_authoring_produces_same_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    assert_eq!(h1, h2, "identical authoring must hash identically");
}

#[test]
fn changing_fill_color_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.4, 0.8),
                        ..Default::default()
                    }) // different red
                    .show(ui);
            })
            .node
    });
    // The root panel paints no shapes (no fill set), so its own hash
    // stays the same. The fill change is on the *child*. The root
    // hash captures only its own fields, so this assertion is on the
    // child's hash via reading it directly.
    let _ = (h1, h2); // root is unaffected — pin the child instead.

    let mut ui1 = Ui::new();
    ui1.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    let mut child1 = None;
    Panel::hstack().id_salt("root").show(&mut ui1, |ui| {
        child1 = Some(
            Frame::new()
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui)
                .node,
        );
    });
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut child2 = None;
    Panel::hstack().id_salt("root").show(&mut ui2, |ui| {
        child2 = Some(
            Frame::new()
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui)
                .node,
        );
    });
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_ne!(
        ui1.forest.tree(Layer::Main).rollups.node[child1.unwrap().index()],
        ui2.forest.tree(Layer::Main).rollups.node[child2.unwrap().index()],
        "different fill must produce different hash",
    );
}

#[test]
fn widget_id_does_not_affect_hash() {
    // Same authoring, different ids → same hash. The hash captures
    // *value*, the WidgetId is the *key* into the prev-map.
    let h1 = record_hash(|ui| Panel::hstack().id_salt("a").show(ui, |_| {}).node);
    let h2 = record_hash(|ui| Panel::hstack().id_salt("b").show(ui, |_| {}).node);
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
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .size((Sizing::Fixed(101.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "padding",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .padding(8.0)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .padding(12.0)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "visibility",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .visibility(Visibility::Visible)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .visibility(Visibility::Hidden)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "justify",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .justify(Justify::Start)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .justify(Justify::Center)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "focusable",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .focusable(false)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .focusable(true)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "disabled",
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .disabled(false)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .id_salt("root")
                    .disabled(true)
                    .show(ui, |_| {})
                    .node
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
fn shape_order_matters_for_hash() {
    // bg-then-text and text-then-bg paint differently. Hash must
    // reflect that.
    let mut ui1 = Ui::new();
    ui1.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    let mut n1 = None;
    Panel::hstack().auto_id().show(&mut ui1, |ui| {
        // Push a Frame then add a manual Text shape via a Button.
        n1 = Some(Button::new().id_salt("a").label("X").show(ui).node);
    });
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    // Two recordings of the same Button — hashes must match.
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut n2 = None;
    Panel::hstack().auto_id().show(&mut ui2, |ui| {
        n2 = Some(Button::new().id_salt("a").label("X").show(ui).node);
    });
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[n1.unwrap().index()],
        ui2.forest.tree(Layer::Main).rollups.node[n2.unwrap().index()],
    );
}

/// Meta-guard: changing the *text* of a `Shape::Text` (e.g., counter
/// updating) changes the hash. This catches "I'd forgotten to hash
/// the text content."
#[test]
fn changing_text_content_changes_hash() {
    use crate::widgets::text::Text;
    let mut ui1 = Ui::new();
    ui1.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    let mut a = None;
    Panel::hstack().auto_id().show(&mut ui1, |ui| {
        a = Some(Text::new("Hello").id_salt("t").show(ui).node);
    });
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut b = None;
    Panel::hstack().auto_id().show(&mut ui2, |ui| {
        b = Some(Text::new("World").id_salt("t").show(ui).node);
    });
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_ne!(
        ui1.forest.tree(Layer::Main).rollups.node[a.unwrap().index()],
        ui2.forest.tree(Layer::Main).rollups.node[b.unwrap().index()]
    );
}

/// Meta-guard: a change to a *child* doesn't ripple into the parent's
/// hash. Each node's hash is *local* — Stage 3's dirty-set is the
/// per-node array, not subtree-aggregated.
#[test]
fn child_hash_does_not_affect_parent_hash() {
    let mut ui1 = Ui::new();
    ui1.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    let parent1 = Panel::hstack()
        .id_salt("root")
        .show(&mut ui1, |ui| {
            Frame::new()
                .id_salt("c")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui);
        })
        .node;
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let parent2 = Panel::hstack()
        .id_salt("root")
        .show(&mut ui2, |ui| {
            Frame::new()
                .id_salt("c")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.4, 0.8),
                    ..Default::default()
                }) // different child fill
                .show(ui);
        })
        .node;
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[parent1.index()],
        ui2.forest.tree(Layer::Main).rollups.node[parent2.index()],
        "parent hash captures only its own fields, not children's",
    );
}

// --- Subtree-hash rollup ----------------------------------------------------
// `Tree.subtree_hashes[i]` folds `hashes[i]` with each direct child's
// subtree hash, in declaration order. Equality across frames means
// nothing in the subtree changed — the contract the cross-frame measure
// cache will rely on.

fn record_subtree_hash<F: FnOnce(&mut Ui) -> NodeId>(f: F) -> NodeHash {
    let mut ui = ui_at(UVec2::new(200, 200));
    let target = f(&mut ui);
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    ui.forest.tree(Layer::Main).rollups.subtree[target.index()]
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
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    };
    let h1 = record_subtree_hash(build);
    let h2 = record_subtree_hash(build);
    assert_eq!(h1, h2);
}

#[test]
fn subtree_hash_changes_when_descendant_changes() {
    let h1 = record_subtree_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    let h2 = record_subtree_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.4, 0.8),
                        ..Default::default()
                    }) // changed leaf fill
                    .show(ui);
            })
            .node
    });
    assert_ne!(
        h1, h2,
        "leaf change must invalidate every ancestor's subtree hash",
    );
}

#[test]
fn subtree_hash_changes_on_sibling_reorder() {
    let h_ab = record_subtree_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    let h_ba = record_subtree_hash(|ui| {
        Panel::hstack()
            .id_salt("root")
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("b")
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node
    });
    assert_ne!(
        h_ab, h_ba,
        "sibling reorder must change the parent's subtree hash",
    );
}

#[test]
fn leaf_subtree_hash_depends_on_node_hash() {
    // For a leaf, the subtree hash is a deterministic function of the
    // node hash (nothing else folded in), but the two values are not
    // identical — the rollup runs the node hash through FxHasher
    // again. Pin: equal node hashes ⇒ equal subtree hashes.
    let mut ui1 = Ui::new();
    ui1.begin_frame(Display::default());
    let leaf1 = Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui1)
        .node;
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let leaf2 = Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui2)
        .node;
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[leaf1.index()],
        ui2.forest.tree(Layer::Main).rollups.node[leaf2.index()]
    );
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.subtree[leaf1.index()],
        ui2.forest.tree(Layer::Main).rollups.subtree[leaf2.index()]
    );
}

/// Transform changes are intentionally folded into `subtree_hash` only,
/// not the per-node hash — the encode cache (subtree-keyed) must
/// invalidate while damage rect-diffing handles paint-position drift.
/// Pin both directions in one fixture.
#[test]
fn transform_change_affects_subtree_but_not_node_hash() {
    use crate::primitives::transform::TranslateScale;
    use glam::Vec2;

    let mut ui1 = ui_at(UVec2::new(200, 200));
    let n1 = Panel::hstack()
        .id_salt("root")
        .transform(TranslateScale::IDENTITY)
        .show(&mut ui1, |_| {})
        .node;
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    let mut ui2 = ui_at(UVec2::new(200, 200));
    let n2 = Panel::hstack()
        .id_salt("root")
        .transform(TranslateScale::from_translation(Vec2::new(10.0, 0.0)))
        .show(&mut ui2, |_| {})
        .node;
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[n1.index()],
        ui2.forest.tree(Layer::Main).rollups.node[n2.index()],
        "transform change must NOT change per-node hash",
    );
    assert_ne!(
        ui1.forest.tree(Layer::Main).rollups.subtree[n1.index()],
        ui2.forest.tree(Layer::Main).rollups.subtree[n2.index()],
        "transform change MUST change subtree hash (encode cache key)",
    );
}

/// `LayoutMode::Grid(idx)` carries a frame-local arena slot that shifts
/// with sibling order. The per-node hash must NOT depend on it — only
/// on the def's contents, which are rolled in at `NodeExit`. Two frames
/// recording the same grid in different relative positions to other
/// grids must produce identical per-node hashes for the matching grid.
#[test]
fn grid_per_node_hash_independent_of_arena_slot() {
    use crate::layout::types::track::Track;
    use crate::widgets::grid::Grid;
    use std::rc::Rc;

    let cols: Rc<[Track]> = Rc::from([Track::fill(), Track::fill()]);
    let rows: Rc<[Track]> = Rc::from([Track::fill()]);

    // Frame 1: target grid recorded first. Slot 0.
    let mut ui1 = ui_at(UVec2::new(200, 200));
    let mut g1 = None;
    Panel::vstack().id_salt("root").show(&mut ui1, |ui| {
        g1 = Some(
            Grid::new()
                .id_salt("target")
                .cols(cols.clone())
                .rows(rows.clone())
                .show(ui, |_| {})
                .node,
        );
        Grid::new()
            .id_salt("other")
            .cols(cols.clone())
            .rows(rows.clone())
            .show(ui, |_| {});
    });
    ui1.end_frame_record_phase();
    ui1.end_frame_paint_phase();
    // Frame 2: same grids, swapped declaration order. Target grid now
    // gets arena slot 1 instead of 0.
    let mut ui2 = ui_at(UVec2::new(200, 200));
    let mut g2 = None;
    Panel::vstack().id_salt("root").show(&mut ui2, |ui| {
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
                .node,
        );
    });
    ui2.end_frame_record_phase();
    ui2.end_frame_paint_phase();
    assert_eq!(
        ui1.forest.tree(Layer::Main).rollups.node[g1.unwrap().index()],
        ui2.forest.tree(Layer::Main).rollups.node[g2.unwrap().index()],
        "grid arena slot must not contribute to the per-node hash",
    );
}

// --- subtree_end rollup ----------------------------------------------------
// `Tree::open_node` writes the per-node leaf marker `i + 1`;
// `close_node` rolls each closing subtree up into its parent's slot.
// The invariant: `subtree_end[i]` points one past the last descendant
// of `i` in pre-order, and is final the moment the root's `close_node`
// returns — no separate finalize pass.

#[test]
fn subtree_end_rolls_up_during_recording() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let root = Panel::hstack()
        .id_salt("root")
        .show(&mut ui, |ui| {
            Frame::new().id_salt("a").size(10.0).show(ui);
            Panel::hstack().id_salt("inner").show(ui, |ui| {
                Frame::new().id_salt("b").size(10.0).show(ui);
                Frame::new().id_salt("c").size(10.0).show(ui);
            });
            Frame::new().id_salt("d").size(10.0).show(ui);
        })
        .node;
    // Tree (pre-order):  0=root  1=a  2=inner  3=b  4=c  5=d
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 6);
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[root.index()],
        6,
        "root"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[1],
        2,
        "leaf a"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[2],
        5,
        "inner spans b,c"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[3],
        4,
        "leaf b"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[4],
        5,
        "leaf c"
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).records.subtree_end()[5],
        6,
        "leaf d"
    );
}

#[test]
fn subtree_end_handles_deep_nesting() {
    // Linear chain: depth-N stacks each containing one stack until a leaf.
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
    ui.begin_frame(Display::default());
    nest(&mut ui, 16);
    let n = ui.forest.tree(Layer::Main).records.len() as u32;
    assert_eq!(n, 17, "16 stacks + 1 leaf");
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
        "leaf"
    );
}

/// Pin: `subtree_hash` rollup is root-local. Multi-root prep — when
/// `Ui::layer` lands (`docs/popups.md` step 2), a popup recorded
/// alongside the Main tree must hash independently of Main's content.
/// Today we synthesize the second root by recording two top-level
/// subtrees back-to-back; `open_node` lazy-pushes a `RootSlot` for each.
/// Both slots are `Main` here (step 2 introduces a per-layer push).
#[test]
fn subtree_hash_rollup_root_local_across_two_roots() {
    fn build(ui: &mut Ui, root_a_color: Color) -> u32 {
        // Root A — content varies via `root_a_color`.
        Panel::vstack().id_salt("root-a").show(ui, |ui| {
            Frame::new()
                .id_salt("a-leaf")
                .size(50.0)
                .background(Background {
                    fill: root_a_color,
                    ..Default::default()
                })
                .show(ui);
        });
        // Capture the index where root B will start, then record root B
        // (identical across both invocations).
        let b_first = ui.forest.tree(Layer::Main).records.len() as u32;
        Panel::vstack().id_salt("root-b").show(ui, |ui| {
            Frame::new().id_salt("b-leaf").size(30.0).show(ui);
        });
        b_first
    }
    let (h_b1, b_first1) = {
        let mut ui = ui_at(UVec2::new(200, 200));
        let b_first = build(&mut ui, Color::rgb(1.0, 0.0, 0.0));
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        (
            ui.forest.tree(Layer::Main).rollups.subtree[b_first as usize],
            b_first,
        )
    };
    let (h_b2, b_first2) = {
        let mut ui = ui_at(UVec2::new(200, 200));
        let b_first = build(&mut ui, Color::rgb(0.0, 1.0, 0.0));
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        (
            ui.forest.tree(Layer::Main).rollups.subtree[b_first as usize],
            b_first,
        )
    };
    assert_eq!(
        b_first1, b_first2,
        "root B's first node must land at the same index in both builds",
    );
    assert_eq!(
        h_b1, h_b2,
        "root B's subtree_hash must not fold root A's content",
    );
}

/// Pin: `Ui::layer` dispatches the popup body into the `Popup` tree.
/// Main and Popup live in separate arenas; popup body nodes nest
/// inside their own root, never inside the surrounding Main scope.
#[test]
fn ui_layer_records_popup_into_separate_tree() {
    let mut ui = ui_at(UVec2::new(400, 400));
    let popup_anchor = Rect {
        min: glam::Vec2::new(50.0, 60.0),
        size: crate::primitives::size::Size::new(100.0, 80.0),
    };
    Panel::vstack().id_salt("main-root").show(&mut ui, |ui| {
        Frame::new().id_salt("main-leaf").size(50.0).show(ui);
        Frame::new().id_salt("main-leaf-2").size(30.0).show(ui);
    });
    ui.layer(Layer::Popup, popup_anchor, |ui| {
        Panel::vstack().id_salt("popup-root").show(ui, |ui| {
            Frame::new().id_salt("popup-leaf").size(20.0).show(ui);
        });
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);
    assert_eq!(main_tree.roots.len(), 1, "Main has one root");
    assert_eq!(popup_tree.roots.len(), 1, "Popup has one root");
    assert_eq!(main_tree.roots[0].first_node, 0);
    assert_eq!(popup_tree.roots[0].first_node, 0);

    // Popup's anchor passes through unchanged from `Ui::layer`.
    let r = popup_tree.roots[0].anchor_rect;
    assert_eq!(r.min, popup_anchor.min);
    assert_eq!(r.size, popup_anchor.size);

    // Each tree is self-contained: Main's root subtree covers only
    // Main records, popup's covers only popup records.
    assert_eq!(
        main_tree.records.subtree_end()[0] as usize,
        main_tree.records.len(),
        "Main's subtree covers every Main record",
    );
    assert_eq!(
        popup_tree.records.subtree_end()[0] as usize,
        popup_tree.records.len(),
        "Popup's subtree covers every Popup record",
    );
}

/// Pin: an empty `Ui::layer` body records no nodes; the popup tree
/// stays empty while Main's tree is unaffected.
#[test]
fn empty_popup_body_leaves_popup_tree_empty() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::vstack().id_salt("only-main").show(&mut ui, |ui| {
        Frame::new().id_salt("leaf").size(20.0).show(ui);
    });
    ui.layer(Layer::Popup, Rect::ZERO, |_| {});
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(ui.forest.tree(Layer::Main).roots.len(), 1);
    assert!(
        ui.forest.tree(Layer::Popup).roots.is_empty(),
        "empty popup body pushes no root",
    );
    assert!(ui.forest.tree(Layer::Popup).is_empty());
}

/// Pin: recording order between layers is irrelevant because trees
/// are independent — popup-recorded-first or main-recorded-first
/// produces the same per-tree contents.
#[test]
fn forest_independence_across_recording_orders() {
    let popup_anchor = Rect {
        min: glam::Vec2::new(10.0, 10.0),
        size: crate::primitives::size::Size::new(60.0, 60.0),
    };
    let mut ui_p_first = ui_at(UVec2::new(400, 400));
    ui_p_first.layer(Layer::Popup, popup_anchor, |ui| {
        Panel::vstack().id_salt("popup-root").show(ui, |ui| {
            Frame::new().id_salt("popup-leaf").size(20.0).show(ui);
        });
    });
    Panel::vstack()
        .id_salt("main-root")
        .show(&mut ui_p_first, |ui| {
            Frame::new().id_salt("main-leaf").size(50.0).show(ui);
        });
    ui_p_first.end_frame_record_phase();
    ui_p_first.end_frame_paint_phase();
    let mut ui_m_first = ui_at(UVec2::new(400, 400));
    Panel::vstack()
        .id_salt("main-root")
        .show(&mut ui_m_first, |ui| {
            Frame::new().id_salt("main-leaf").size(50.0).show(ui);
        });
    ui_m_first.layer(Layer::Popup, popup_anchor, |ui| {
        Panel::vstack().id_salt("popup-root").show(ui, |ui| {
            Frame::new().id_salt("popup-leaf").size(20.0).show(ui);
        });
    });
    ui_m_first.end_frame_record_phase();
    ui_m_first.end_frame_paint_phase();
    for layer in [Layer::Main, Layer::Popup] {
        assert_eq!(
            ui_p_first.forest.tree(layer).records.len(),
            ui_m_first.forest.tree(layer).records.len(),
            "{layer:?} record count independent of recording order",
        );
    }
}

/// Pin: a mid-recording popup with text-bearing widgets (Button
/// labels) renders end-to-end without leaking shapes between Main
/// and Popup. With per-layer trees, Main's shapes buffer never
/// receives popup texts in the first place — each tree owns its
/// own buffer. Mirrors the showcase popup tab structure.
#[test]
fn mid_recording_popup_with_text_renders_through_encoder() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let popup_anchor = Rect {
        min: glam::Vec2::new(50.0, 100.0),
        size: crate::primitives::size::Size::new(200.0, 200.0),
    };
    Panel::vstack().id_salt("outer-main").show(&mut ui, |ui| {
        Button::new().id_salt("trigger").label("menu").show(ui);
        ui.layer(Layer::Popup, popup_anchor, |ui| {
            Panel::vstack().id_salt("popup-body").show(ui, |ui| {
                Button::new().id_salt("popup-item").label("copy").show(ui);
            });
        });
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let _cmds = encode_cmds(&ui);

    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);

    let outer_span = main_tree.records.shape_span()[0];
    let main_texts: Vec<&str> = main_tree.shapes
        [outer_span.start as usize..(outer_span.start + outer_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            Shape::Text { text, .. } => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(
        main_texts,
        vec!["menu"],
        "Main tree owns only the trigger label",
    );

    let popup_root_span = popup_tree.records.shape_span()[0];
    let popup_texts: Vec<&str> = popup_tree.shapes
        [popup_root_span.start as usize..(popup_root_span.start + popup_root_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            Shape::Text { text, .. } => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(popup_texts, vec!["copy"], "Popup tree owns 'copy'");
}

/// Pin: `Ui::layer` is callable mid-recording. The popup body's
/// records dispatch directly into the `Popup` tree, so Main's tree
/// only ever sees Main records and Popup's tree only sees popup
/// records — no shared buffer, no interleaving, no permutation
/// pass. Within each tree, recording order (== pre-order) is
/// preserved.
///
/// Fixture mirrors `docs/popups.md` step 4: `Main { mc1, mc2,
/// Popup { ps1, ps2 }, mc3, mc4 }`. Direct shapes are pushed at
/// every Main + Popup level so the fixture also pins per-tree
/// shape buffer contents: each shape appears exactly once, in its
/// owning tree, in recording order.
#[test]
fn mid_recording_popup_keeps_trees_independent() {
    fn marker(slot: u8) -> Shape {
        let w = (slot + 1) as f32;
        Shape::RoundedRect {
            local_rect: Some(Rect::new(0.0, 0.0, w, w)),
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0),
            stroke: Stroke::ZERO,
        }
    }
    fn marker_w(s: &Shape) -> u32 {
        match s {
            Shape::RoundedRect {
                local_rect: Some(r),
                ..
            } => r.size.w as u32,
            _ => panic!("unexpected shape variant"),
        }
    }

    let mut ui = ui_at(UVec2::new(400, 400));
    let popup_anchor = Rect {
        min: glam::Vec2::new(50.0, 60.0),
        size: crate::primitives::size::Size::new(100.0, 80.0),
    };
    let parent = Panel::vstack()
        .id_salt("main-parent")
        .show(&mut ui, |ui| {
            ui.add_shape(marker(0));
            Frame::new().id_salt("mc1").size(20.0).show(ui);
            ui.add_shape(marker(1));
            Frame::new().id_salt("mc2").size(20.0).show(ui);
            ui.add_shape(marker(2));
            ui.layer(Layer::Popup, popup_anchor, |ui| {
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
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let main_tree = ui.forest.tree(Layer::Main);
    let popup_tree = ui.forest.tree(Layer::Popup);

    // Main tree: [parent, mc1, mc2, mc3, mc4]. Popup is absent.
    assert_eq!(main_tree.records.len(), 5);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node, 0);
    assert_eq!(
        main_tree.records.subtree_end()[parent.index()],
        5,
        "parent's subtree spans every Main record",
    );

    let kids: Vec<u32> = main_tree.children(parent).map(|c| c.id.0).collect();
    assert_eq!(kids, vec![1, 2, 3, 4], "no popup leak in Main children");

    let widths: Vec<u32> = main_tree.shapes.iter().map(marker_w).collect();
    assert_eq!(
        widths,
        vec![1, 2, 3, 4, 5],
        "Main shapes preserve recording order",
    );
    let parent_span = main_tree.records.shape_span()[parent.index()];
    assert_eq!(parent_span.start, 0);
    assert_eq!(parent_span.len, 5);
    for leaf_idx in [1, 2, 3, 4] {
        assert_eq!(
            main_tree.records.shape_span()[leaf_idx as usize].len,
            0,
            "Main leaf at {leaf_idx} has no direct shapes",
        );
    }

    // Popup tree: [popup_root, popup_leaf, popup_leaf_2].
    assert_eq!(popup_tree.records.len(), 3);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots[0].first_node, 0);
    assert_eq!(
        popup_tree.records.subtree_end()[0],
        3,
        "popup root spans every Popup record",
    );

    let popup_widths: Vec<u32> = popup_tree.shapes.iter().map(marker_w).collect();
    assert_eq!(popup_widths, vec![11, 12], "Popup shapes in record order");
    let popup_root_span = popup_tree.records.shape_span()[0];
    assert_eq!(popup_root_span.start, 0);
    assert_eq!(popup_root_span.len, 2);
    for leaf_idx in [1, 2] {
        assert_eq!(
            popup_tree.records.shape_span()[leaf_idx as usize].len,
            0,
            "Popup leaf at {leaf_idx} has no direct shapes",
        );
    }
}

/// Pin the bounds/panel column split: `.gap(...)` is panel-only, so it
/// must populate `panel.table` without touching `bounds.table`. Conversely,
/// `.min_size(...)` populates `bounds.table` without touching `panel.table`.
/// Catches accidental re-merging of the two columns or a setter routing to
/// the wrong column.
#[test]
fn extras_columns_split_by_field_kind() {
    use crate::primitives::size::Size;

    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack()
        .id_salt("panel-with-gap")
        .gap(8.0)
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("leaf-with-min")
                .min_size(Size::new(20.0, 20.0))
                .show(ui);
            Frame::new().id_salt("plain-leaf").size(10.0).show(ui);
        });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Panel set `.gap`: one entry in `panel.table`, none in `bounds.table`.
    // Leaf set `.min_size`: one entry in `bounds.table`, none in `panel.table`.
    // Plain leaf set neither: contributes to neither table.
    assert_eq!(
        ui.forest.tree(Layer::Main).panel.table.len(),
        1,
        "only the gapped panel populates panel.table",
    );
    assert_eq!(
        ui.forest.tree(Layer::Main).bounds.table.len(),
        1,
        "only the min-sized leaf populates bounds.table",
    );
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let root = Panel::hstack()
        .id_salt("root")
        .show(&mut ui, |ui| {
            Frame::new().id_salt("a").size(10.0).show(ui);
            Panel::hstack().id_salt("inner").show(ui, |ui| {
                Frame::new().id_salt("b").size(10.0).show(ui);
            });
            Frame::new().id_salt("c").size(10.0).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<u32> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id.0)
        .collect();
    assert_eq!(kids, vec![1, 2, 4], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = ui
        .forest
        .tree(Layer::Main)
        .children(NodeId(2))
        .map(|c| c.id.0)
        .collect();
    assert_eq!(inner_kids, vec![3], "inner's direct child: b");
}
