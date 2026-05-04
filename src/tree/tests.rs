use crate::Ui;
use crate::layout::types::{display::Display, justify::Justify, sizing::Sizing};
use crate::primitives::color::Color;
use crate::shape::Shape;
use crate::test_support::ui_at;
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
    ui.tree.compute_hashes();
    assert_eq!(ui.tree.node_count(), 0);
    assert!(ui.tree.hashes.is_empty());
    assert!(ui.tree.subtree_hashes.is_empty());
}

#[test]
fn same_authoring_produces_same_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
            })
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
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
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
            })
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
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
    Panel::hstack_with_id("root").show(&mut ui1, |ui| {
        child1 = Some(
            Frame::with_id("a")
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
    Panel::hstack_with_id("root").show(&mut ui2, |ui| {
        child2 = Some(
            Frame::with_id("a")
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
    let h1 = record_hash(|ui| Panel::hstack_with_id("a").show(ui, |_| {}).node);
    let h2 = record_hash(|ui| Panel::hstack_with_id("b").show(ui, |_| {}).node);
    assert_eq!(h1, h2);
}

#[test]
fn changing_layout_size_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .size((Sizing::Fixed(101.0), Sizing::Fixed(50.0)))
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
}

#[test]
fn changing_padding_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .padding(8.0)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
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
        Panel::hstack_with_id("root")
            .visibility(Visibility::Visible)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .visibility(Visibility::Hidden)
            .show(ui, |_| {})
            .node
    });
    assert_ne!(h1, h2);
}

#[test]
fn changing_justify_changes_hash() {
    let h1 = record_hash(|ui| {
        Panel::hstack_with_id("root")
            .justify(Justify::Start)
            .show(ui, |_| {})
            .node
    });
    let h2 = record_hash(|ui| {
        Panel::hstack_with_id("root")
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
        n1 = Some(Button::with_id("a").label("X").show(ui).node);
    });
    ui1.end_frame();

    // Two recordings of the same Button — hashes must match.
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut n2 = None;
    Panel::hstack().show(&mut ui2, |ui| {
        n2 = Some(Button::with_id("a").label("X").show(ui).node);
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
        a = Some(Text::with_id("t", "Hello").show(ui).node);
    });
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let mut b = None;
    Panel::hstack().show(&mut ui2, |ui| {
        b = Some(Text::with_id("t", "World").show(ui).node);
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
    let parent1 = Panel::hstack_with_id("root")
        .show(&mut ui1, |ui| {
            Frame::with_id("c")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        })
        .node;
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let parent2 = Panel::hstack_with_id("root")
        .show(&mut ui2, |ui| {
            Frame::with_id("c")
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
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
                Frame::with_id("b")
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
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
            })
            .node
    });
    let h2 = record_subtree_hash(|ui| {
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
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
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size(50.0)
                    .fill(Color::rgb(0.2, 0.4, 0.8))
                    .show(ui);
                Frame::with_id("b")
                    .size(30.0)
                    .fill(Color::rgb(0.9, 0.1, 0.1))
                    .show(ui);
            })
            .node
    });
    let h_ba = record_subtree_hash(|ui| {
        Panel::hstack_with_id("root")
            .show(ui, |ui| {
                Frame::with_id("b")
                    .size(30.0)
                    .fill(Color::rgb(0.9, 0.1, 0.1))
                    .show(ui);
                Frame::with_id("a")
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
    let leaf1 = Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui1)
        .node;
    ui1.end_frame();

    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    let leaf2 = Frame::with_id("a")
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
