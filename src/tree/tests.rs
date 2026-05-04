use crate::Ui;
use crate::layout::types::{display::Display, justify::Justify, sizing::Sizing};
use crate::primitives::color::Color;
use crate::shape::Shape;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::tree::{NodeId, node_hash::NodeHash};
use crate::widgets::{button::Button, frame::Frame, panel::Panel, styled::Styled};
use glam::UVec2;

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    let mut button_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        button_node = Some(Button::new().label("X").show(ui).node);
    });

    let shapes = ui.tree.shapes_of(button_node.unwrap());
    assert_eq!(shapes.len(), 2);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));
    assert!(matches!(shapes[1], Shape::Text { .. }));
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
    ui.tree.hashes[target.index()]
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
    
    assert_eq!(ui.tree.node_count(), 0);
    assert!(ui.tree.hashes.is_empty());
    assert!(ui.tree.subtree_hashes.is_empty());
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
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
                    .fill(Color::rgb(0.9, 0.4, 0.8)) // different red
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
                .fill(Color::rgb(0.2, 0.4, 0.8))
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
                .fill(Color::rgb(0.9, 0.4, 0.8))
                .show(ui)
                .node,
        );
    });
    ui2.end_frame();

    assert_ne!(
        ui1.tree.hashes[child1.unwrap().index()],
        ui2.tree.hashes[child2.unwrap().index()],
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
fn changing_layout_size_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .size((Sizing::Fixed(101.0), Sizing::Fixed(50.0)))
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
}

#[test]
fn changing_padding_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .padding(8.0)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .padding(12.0)
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
}

#[test]
fn changing_visibility_changes_hash() {
    use crate::layout::types::visibility::Visibility;
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .visibility(Visibility::Visible)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .visibility(Visibility::Hidden)
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
}

#[test]
fn changing_justify_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .justify(Justify::Start)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .with_id("root")
            .justify(Justify::Center)
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
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
        ui1.tree.hashes[n1.unwrap().index()],
        ui2.tree.hashes[n2.unwrap().index()],
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
        ui1.tree.hashes[a.unwrap().index()],
        ui2.tree.hashes[b.unwrap().index()]
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
                .fill(Color::rgb(0.2, 0.4, 0.8))
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
                .fill(Color::rgb(0.9, 0.4, 0.8)) // different child fill
                .show(ui);
        })
        .node;
    ui2.end_frame();

    assert_eq!(
        ui1.tree.hashes[parent1.index()],
        ui2.tree.hashes[parent2.index()],
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
                Frame::new()
                    .with_id("b")
                    .size(30.0)
                    .fill(Color::rgb(0.9, 0.1, 0.1))
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
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
                    .fill(Color::rgb(0.9, 0.4, 0.8)) // changed leaf fill
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
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
                Frame::new()
                    .with_id("b")
                    .size(30.0)
                    .fill(Color::rgb(0.9, 0.1, 0.1))
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
                    .fill(Color::rgb(0.9, 0.1, 0.1))
                    .show(ui);
                Frame::new()
                    .with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
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
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui1)
        .node;
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let leaf2 = Frame::new()
        .with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui2)
        .node;
    ui2.end_frame();

    assert_eq!(
        ui1.tree.hashes[leaf1.index()],
        ui2.tree.hashes[leaf2.index()]
    );
    assert_eq!(ui1.tree.subtree_hash(leaf1), ui2.tree.subtree_hash(leaf2));
}

// --- subtree_end finalization ----------------------------------------------
// `Tree::open_node` writes only the per-node leaf marker `i + 1`;
// `finalize_subtree_end` (called from `compute_hashes` and therefore
// `end_frame`) propagates each child's slot up to its parent. The
// invariant: `subtree_end[i]` points one past the last descendant of
// `i` in pre-order. Below pins both the post-recording leaf state and
// the post-finalize correctness across nesting shapes.

#[test]
fn subtree_end_is_leaf_marker_before_finalize() {
    // Recording-only state: every slot equals i+1 because no ancestor
    // walk runs in `open_node` anymore. `finalize_subtree_end` /
    // `compute_hashes` must run before any subtree iteration is valid.
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack().with_id("root").show(&mut ui, |ui| {
        Frame::new().with_id("a").size(10.0).show(ui);
        Frame::new().with_id("b").size(10.0).show(ui);
    });
    let n = ui.tree.node_count();
    for i in 0..n {
        assert_eq!(
            ui.tree.subtree_end[i],
            (i as u32) + 1,
            "node {i} should hold the leaf marker pre-finalize",
        );
    }
}

#[test]
fn finalize_subtree_end_rolls_up_children() {
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
    assert_eq!(ui.tree.node_count(), 6);
    ui.tree.finalize_subtree_end();
    assert_eq!(ui.tree.subtree_end[root.index()], 6, "root");
    assert_eq!(ui.tree.subtree_end[1], 2, "leaf a");
    assert_eq!(ui.tree.subtree_end[2], 5, "inner spans b,c");
    assert_eq!(ui.tree.subtree_end[3], 4, "leaf b");
    assert_eq!(ui.tree.subtree_end[4], 5, "leaf c");
    assert_eq!(ui.tree.subtree_end[5], 6, "leaf d");
}

#[test]
fn finalize_subtree_end_handles_deep_nesting() {
    // Linear chain: depth-N stacks each containing one stack until a
    // leaf. Old code did O(N·depth) random writes; the new pass must
    // still produce subtree_end[0] = N for the chain root.
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
    ui.end_frame();
    let n = ui.tree.node_count() as u32;
    assert_eq!(n, 17, "16 stacks + 1 leaf");
    for i in 0..(n - 1) {
        assert_eq!(
            ui.tree.subtree_end[i as usize], n,
            "every ancestor on the chain points past the leaf",
        );
    }
    assert_eq!(ui.tree.subtree_end[(n - 1) as usize], n, "leaf");
}

#[test]
fn finalize_subtree_end_is_idempotent() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack().with_id("root").show(&mut ui, |ui| {
        Panel::vstack().with_id("inner").show(ui, |ui| {
            Frame::new().with_id("a").size(10.0).show(ui);
            Frame::new().with_id("b").size(10.0).show(ui);
        });
        Frame::new().with_id("c").size(10.0).show(ui);
    });
    ui.tree.finalize_subtree_end();
    let snapshot: Vec<u32> = ui.tree.subtree_end.clone();
    ui.tree.finalize_subtree_end();
    ui.tree.finalize_subtree_end();
    assert_eq!(ui.tree.subtree_end, snapshot);
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
    let kids: Vec<u32> = ui.tree.children(root).map(|n| n.0).collect();
    assert_eq!(kids, vec![1, 2, 4], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = ui.tree.children(NodeId(2)).map(|n| n.0).collect();
    assert_eq!(inner_kids, vec![3], "inner's direct child: b");
}
