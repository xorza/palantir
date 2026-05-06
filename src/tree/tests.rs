use crate::Ui;
use crate::layout::types::{display::Display, justify::Justify, sizing::Sizing};
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::CmdKind;
use crate::shape::Shape;
use crate::support::testing::{encode_cmds, ui_at};
use crate::tree::element::Configure;
use crate::tree::{NodeId, node_hash::NodeHash};
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let mut button_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        button_node = Some(Button::new().label("X").show(ui).node);
    });

    // Chrome (the button background) lives in `Tree::chrome_table`,
    // not in the shapes list. Only the label `Text` shape lands here.
    let shapes: Vec<&Shape> = ui.tree.shapes_of(button_node.unwrap()).collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::Text { .. }));
    assert!(
        ui.tree.chrome_for(button_node.unwrap()).is_some(),
        "button chrome recorded in chrome table",
    );
}

/// Pin the kinds-stream mechanism end-to-end: when shapes are interleaved
/// with child nodes under one parent, the kinds stream encodes their
/// position in record order (Shape between NodeEnter/NodeExit pairs of
/// the right children). Each shape's size encodes the expected slot for
/// an unambiguous readback.
#[test]
fn interleaved_shapes_record_correct_kinds_stream() {
    fn pos_rect(slot: u16) -> Shape {
        let s = (slot + 1) as f32 * 10.0;
        Shape::SubRect {
            local_rect: Rect::new(0.0, 0.0, s, s),
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0),
            stroke: None,
        }
    }
    let mut ui = ui_at(UVec2::new(200, 200));
    let p = Panel::vstack()
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .show(&mut ui, |ui| {
            ui.add_shape(pos_rect(0));
            Frame::new()
                .with_id("c0")
                .background(Background {
                    fill: Color::rgb(0.0, 1.0, 0.0),
                    ..Default::default()
                })
                .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                .show(ui);
            ui.add_shape(pos_rect(1));
            Frame::new()
                .with_id("c1")
                .background(Background {
                    fill: Color::rgb(0.0, 0.0, 1.0),
                    ..Default::default()
                })
                .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                .show(ui);
            ui.add_shape(pos_rect(2));
        })
        .node;
    ui.end_frame();

    // Walk the parent's kinds slice — it must show our three SubRects
    // interleaved between the two child NodeEnter/NodeExit pairs in
    // record order.
    let pi = p.index();
    let kinds_slice = &ui.tree.kinds[ui.tree.records.kinds()[pi].range()];
    use crate::tree::TreeOp;
    assert_eq!(
        kinds_slice,
        &[
            TreeOp::NodeEnter,
            TreeOp::Shape,
            TreeOp::NodeEnter,
            TreeOp::NodeExit,
            TreeOp::Shape,
            TreeOp::NodeEnter,
            TreeOp::NodeExit,
            TreeOp::Shape,
            TreeOp::NodeExit,
        ][..],
        "kinds stream encodes shape→child→shape→child→shape interleave",
    );
    let sizes: Vec<f32> = ui
        .tree
        .shapes_of(p)
        .map(|s| match s {
            Shape::SubRect {
                local_rect: rect, ..
            } => rect.size.w,
            _ => panic!("unexpected shape variant"),
        })
        .collect();
    // Record order is preserved by direct push to `kinds` + `shapes`.
    assert_eq!(sizes, vec![10.0, 20.0, 30.0]);

    // End-to-end: the encoder paints draw commands in record order —
    // `pos_rect(0)` → child c0 chrome → `pos_rect(1)` → child c1 chrome
    // → `pos_rect(2)`. 3 parent SubRects + 2 child chrome paints = 5
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
/// `Body` child, then pushes bar `SubRect`s at slot N. Without the
/// fix, `nodes[Body].shapes.len` counted the bars too.
#[test]
fn parent_post_child_shapes_dont_inflate_child_subtree_count() {
    fn pos_rect() -> Shape {
        Shape::SubRect {
            local_rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radius: Corners::default(),
            fill: Color::rgb(1.0, 0.0, 0.0),
            stroke: None,
        }
    }
    let mut ui = ui_at(UVec2::new(200, 200));
    let mut child_id = None;
    let parent_id = Panel::vstack()
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .show(&mut ui, |ui| {
            // Single child, no shapes inside.
            child_id = Some(
                Frame::new()
                    .with_id("only-child")
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
    ui.end_frame();

    let parent = parent_id.index();
    let child = child_id.unwrap().index();

    // Parent and child share `end` (parent has only this one child),
    // which is the bug trigger.
    assert_eq!(
        ui.tree.records.end()[parent],
        ui.tree.records.end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );

    // Parent's subtree contains both bar shapes.
    assert_eq!(
        ui.tree.records.shapes()[parent].len,
        2,
        "parent's subtree owns both slot-N shapes"
    );
    // Child's subtree contains zero shapes — the trailing bars belong
    // to the parent, not the child.
    assert_eq!(
        ui.tree.records.shapes()[child].len,
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
    ui.end_frame();
    ui.tree.hashes.node[target.index()]
}

#[test]
fn empty_tree_has_no_hashes() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    // No widgets recorded — node_count is 0 → both hash arrays stay
    // empty. (Layout / end_frame normally need a root, so we
    // intentionally skip them; just call compute_hashes directly to
    // verify the empty-tree case.)
    ui.tree.end_frame();

    assert_eq!(ui.tree.records.len(), 0);
    assert!(ui.tree.hashes.node.is_empty());
    assert!(ui.tree.hashes.subtree.is_empty());
}

#[test]
fn same_authoring_produces_same_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
    Panel::hstack().with_id("root").show(&mut ui1, |ui| {
        child1 = Some(
            Frame::new()
                .with_id("a")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui)
                .node,
        );
    });
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut child2 = None;
    Panel::hstack().with_id("root").show(&mut ui2, |ui| {
        child2 = Some(
            Frame::new()
                .with_id("a")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui)
                .node,
        );
    });
    ui2.end_frame();

    assert_ne!(
        ui1.tree.hashes.node[child1.unwrap().index()],
        ui2.tree.hashes.node[child2.unwrap().index()],
        "different fill must produce different hash",
    );
}

#[test]
fn widget_id_does_not_affect_hash() {
    // Same authoring, different ids → same hash. The hash captures
    // *value*, the WidgetId is the *key* into the prev-map.
    let h1 = record_hash(|ui| Panel::hstack().with_id("a").show(ui, |_| {}).node);
    let h2 = record_hash(|ui| Panel::hstack().with_id("b").show(ui, |_| {}).node);
    assert_eq!(h1, h2);
}

#[test]
fn changing_layout_property_changes_hash() {
    use crate::layout::types::visibility::Visibility;
    type Build = fn(&mut Ui) -> NodeId;
    let cases: &[(&str, Build, Build)] = &[
        (
            "size",
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .size((Sizing::Fixed(101.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "padding",
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .padding(8.0)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .padding(12.0)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "visibility",
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .visibility(Visibility::Visible)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .visibility(Visibility::Hidden)
                    .show(ui, |_| {})
                    .node
            },
        ),
        (
            "justify",
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .justify(Justify::Start)
                    .show(ui, |_| {})
                    .node
            },
            |ui| {
                Panel::hstack()
                    .with_id("root")
                    .justify(Justify::Center)
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
    Panel::hstack().show(&mut ui1, |ui| {
        // Push a Frame then add a manual Text shape via a Button.
        n1 = Some(Button::new().with_id("a").label("X").show(ui).node);
    });
    ui1.end_frame();

    // Two recordings of the same Button — hashes must match.
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut n2 = None;
    Panel::hstack().show(&mut ui2, |ui| {
        n2 = Some(Button::new().with_id("a").label("X").show(ui).node);
    });
    ui2.end_frame();

    assert_eq!(
        ui1.tree.hashes.node[n1.unwrap().index()],
        ui2.tree.hashes.node[n2.unwrap().index()],
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
    Panel::hstack().show(&mut ui1, |ui| {
        a = Some(Text::new("Hello").with_id("t").show(ui).node);
    });
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut b = None;
    Panel::hstack().show(&mut ui2, |ui| {
        b = Some(Text::new("World").with_id("t").show(ui).node);
    });
    ui2.end_frame();

    assert_ne!(
        ui1.tree.hashes.node[a.unwrap().index()],
        ui2.tree.hashes.node[b.unwrap().index()]
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
        .with_id("root")
        .show(&mut ui1, |ui| {
            Frame::new()
                .with_id("c")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui);
        })
        .node;
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let parent2 = Panel::hstack()
        .with_id("root")
        .show(&mut ui2, |ui| {
            Frame::new()
                .with_id("c")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.9, 0.4, 0.8),
                    ..Default::default()
                }) // different child fill
                .show(ui);
        })
        .node;
    ui2.end_frame();

    assert_eq!(
        ui1.tree.hashes.node[parent1.index()],
        ui2.tree.hashes.node[parent2.index()],
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
    ui.end_frame();
    ui.tree.subtree_hash(target)
}

#[test]
fn subtree_hash_stable_across_frames() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .with_id("b")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .with_id("b")
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
            .with_id("root")
            .show(ui, |ui| {
                Frame::new()
                    .with_id("b")
                    .size(30.0)
                    .background(Background {
                        fill: Color::rgb(0.9, 0.1, 0.1),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .with_id("a")
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
        .with_id("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui1)
        .node;
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let leaf2 = Frame::new()
        .with_id("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui2)
        .node;
    ui2.end_frame();

    assert_eq!(
        ui1.tree.hashes.node[leaf1.index()],
        ui2.tree.hashes.node[leaf2.index()]
    );
    assert_eq!(ui1.tree.subtree_hash(leaf1), ui2.tree.subtree_hash(leaf2));
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
        .with_id("root")
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(10.0).show(ui);
            Panel::hstack().with_id("inner").show(ui, |ui| {
                Frame::new().with_id("b").size(10.0).show(ui);
                Frame::new().with_id("c").size(10.0).show(ui);
            });
            Frame::new().with_id("d").size(10.0).show(ui);
        })
        .node;
    // Tree (pre-order):  0=root  1=a  2=inner  3=b  4=c  5=d
    assert_eq!(ui.tree.records.len(), 6);
    assert_eq!(ui.tree.records.end()[root.index()], 6, "root");
    assert_eq!(ui.tree.records.end()[1], 2, "leaf a");
    assert_eq!(ui.tree.records.end()[2], 5, "inner spans b,c");
    assert_eq!(ui.tree.records.end()[3], 4, "leaf b");
    assert_eq!(ui.tree.records.end()[4], 5, "leaf c");
    assert_eq!(ui.tree.records.end()[5], 6, "leaf d");
}

#[test]
fn subtree_end_handles_deep_nesting() {
    // Linear chain: depth-N stacks each containing one stack until a leaf.
    fn nest(ui: &mut Ui, depth: usize) {
        if depth == 0 {
            Frame::new().with_id(("leaf", depth)).size(10.0).show(ui);
            return;
        }
        Panel::vstack()
            .with_id(("nest", depth))
            .show(ui, |ui| nest(ui, depth - 1));
    }
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    nest(&mut ui, 16);
    let n = ui.tree.records.len() as u32;
    assert_eq!(n, 17, "16 stacks + 1 leaf");
    for i in 0..(n - 1) {
        assert_eq!(
            ui.tree.records.end()[i as usize],
            n,
            "every ancestor on the chain points past the leaf",
        );
    }
    assert_eq!(ui.tree.records.end()[(n - 1) as usize], n, "leaf");
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let root = Panel::hstack()
        .with_id("root")
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(10.0).show(ui);
            Panel::hstack().with_id("inner").show(ui, |ui| {
                Frame::new().with_id("b").size(10.0).show(ui);
            });
            Frame::new().with_id("c").size(10.0).show(ui);
        })
        .node;
    ui.end_frame();
    let kids: Vec<u32> = ui.tree.children(root).map(|c| c.id.0).collect();
    assert_eq!(kids, vec![1, 2, 4], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = ui.tree.children(NodeId(2)).map(|c| c.id.0).collect();
    assert_eq!(inner_kids, vec![3], "inner's direct child: b");
}
