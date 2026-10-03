//! What moves a single node's hash, and what deliberately does not.

use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::{SURFACE, record};
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::Vec2;

#[test]
fn empty_tree_has_no_hashes() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|_| {});
    // Synthetic viewport root: present even for an empty user record.
    assert_eq!(h.ui.tree(Layer::Main).records.len(), 1);
    assert_eq!(h.ui.tree(Layer::Main).rollups.node.len(), 1);
    assert_eq!(h.ui.tree(Layer::Main).rollups.subtree.len(), 1);
}

#[test]
fn same_authoring_produces_same_hash() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .show(ui);
            })
            .response
            .node()
    };
    assert_eq!(record(build).node, record(build).node);
    // The hash is the root's own, so the change is to the root.
    let padded = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .padding(4.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .show(ui);
            })
            .response
            .node()
    };
    assert_ne!(
        record(build).node,
        record(padded).node,
        "other authoring, other hash"
    );
}

#[test]
fn polyline_hash_uses_visual_points_and_lowered_colors() {
    fn build(ui: &mut Ui, points: &[Vec2], color: RgbaF32) -> NodeId {
        Panel::canvas()
            .id(WidgetId::from_hash("polyline"))
            .show(ui, |ui| {
                ui.add_shape(Shape::polyline(points, Stroke::new(color, 2.0)));
            })
            .response
            .node()
    }

    let base_points = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
    let noisy_points = [Vec2::new(EPS * 0.5, -EPS * 0.5), Vec2::new(10.0, 0.0)];
    let color_a = RgbaF32::new(0.5, 0.25, 0.75, 1.0);
    let color_b = RgbaF32::new(0.5001, 0.2501, 0.7501, 1.0);
    assert_ne!(color_a, color_b);
    assert_eq!(RgbaF16::from(color_a), RgbaF16::from(color_b));

    let baseline = record(|ui| build(ui, &base_points, color_a)).node;
    assert_eq!(
        baseline,
        record(|ui| build(ui, &noisy_points, color_a)).node,
    );
    assert_eq!(baseline, record(|ui| build(ui, &base_points, color_b)).node);
    // The same comparison does see a move and a colour change it can show.
    let moved = [Vec2::ZERO, Vec2::new(11.0, 0.0)];
    assert_ne!(baseline, record(|ui| build(ui, &moved, color_a)).node);
    let recoloured = RgbaF32::new(0.6, 0.25, 0.75, 1.0);
    assert_ne!(
        baseline,
        record(|ui| build(ui, &base_points, recoloured)).node
    );
}

#[test]
fn changing_fill_color_changes_hash() {
    fn build_child(ui: &mut Ui, fill: RgbaF32) -> NodeId {
        let mut child = None;
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                child = Some(
                    Block::new()
                        .id(WidgetId::from_hash("a"))
                        .size(50.0)
                        .background(Background::fill(fill))
                        .show(ui)
                        .node(),
                );
            });
        child.unwrap()
    }
    let blue = record(|ui| build_child(ui, RgbaF32::srgb(0.2, 0.4, 0.8)));
    let red = record(|ui| build_child(ui, RgbaF32::srgb(0.9, 0.4, 0.8)));
    assert_ne!(blue.node, red.node);
    assert_eq!(
        blue.cascade_static, red.cascade_static,
        "paint-only changes must remain eligible for incremental cascade"
    );
}

#[test]
fn widget_id_only_affects_cascade_static_hash() {
    let build = |id: &'static str| {
        record(move |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash(id))
                .show(ui, |_| {})
                .response
                .node()
        })
    };
    let (a, b) = (build("a"), build("b"));
    assert_eq!(a.node, b.node);
    assert_ne!(
        a.cascade_static, b.cascade_static,
        "identity changes must rebuild cascade hit IDs and its by-id snapshot",
    );
}

#[test]
fn changing_layout_property_changes_hash() {
    use crate::primitives::layout::visibility::Visibility;
    type Build = fn(&mut Ui) -> NodeId;
    let cases: &[(&str, Build, Build)] = &[
        (
            "size",
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                    .show(ui, |_| {})
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::fixed(101.0), Sizing::fixed(50.0)))
                    .show(ui, |_| {})
                    .response
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
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .padding(12.0)
                    .show(ui, |_| {})
                    .response
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
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .visibility(Visibility::Hidden)
                    .show(ui, |_| {})
                    .response
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
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .justify(Justify::Center)
                    .show(ui, |_| {})
                    .response
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
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .focusable(true)
                    .show(ui, |_| {})
                    .response
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
                    .response
                    .node()
            },
            |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("root"))
                    .disabled(true)
                    .show(ui, |_| {})
                    .response
                    .node()
            },
        ),
    ];
    for (label, a, b) in cases {
        let (a, b) = (record(*a), record(*b));
        assert_ne!(a.node, b.node, "case: {label}");
        assert_ne!(
            a.cascade_static, b.cascade_static,
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
    let h1 = record(|ui| build(ui, "Hello")).node;
    let h2 = record(|ui| build(ui, "World")).node;
    assert_ne!(h1, h2);
}

#[test]
fn child_hash_does_not_affect_parent_hash() {
    fn build(ui: &mut Ui, fill: RgbaF32) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("c"))
                    .size(50.0)
                    .background(Background::fill(fill))
                    .show(ui);
            })
            .response
            .node()
    }
    let h1 = record(|ui| build(ui, RgbaF32::srgb(0.2, 0.4, 0.8))).node;
    let h2 = record(|ui| build(ui, RgbaF32::srgb(0.9, 0.4, 0.8))).node;
    assert_eq!(h1, h2, "parent hash captures only its own fields");
}

/// `Tree.shapes.hashes` is parallel to `Tree.shapes.records` after
/// `post_record`: one slot per shape, populated by the existing
/// `compute_rollups` walk so we don't pay a second per-shape sweep.
#[test]
fn shape_hashes_column_sized_to_shape_records() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
            .show(ui, |ui| {
                ui.add_shape(Shape::line(
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                ));
                ui.add_shape(Shape::line(
                    Vec2::new(10.0, 10.0),
                    Vec2::new(20.0, 20.0),
                    Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 1.0),
                ));
            });
    });
    let tree = h.ui.tree(Layer::Main);
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
            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
            .show(ui, |ui| {
                ui.add_shape(Shape::line(
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                ));
            });
    };
    let mut h = UiHarness::new(SURFACE);
    h.frame(build);
    let h0 = h.ui.tree(Layer::Main).shapes.hashes[0];
    h.frame(build);
    let h1 = h.ui.tree(Layer::Main).shapes.hashes[0];
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
    let build = |b_endpoint: Vec2, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
            .show(ui, |ui| {
                ui.add_shape(Shape::line(
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                ));
                ui.add_shape(Shape::line(
                    Vec2::new(5.0, 5.0),
                    b_endpoint,
                    Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 1.0),
                ));
            });
    };
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(Vec2::new(20.0, 20.0), ui));
    let h0_a = h.ui.tree(Layer::Main).shapes.hashes[0];
    let h0_b = h.ui.tree(Layer::Main).shapes.hashes[1];
    h.frame(|ui| build(Vec2::new(30.0, 30.0), ui));
    let h1_a = h.ui.tree(Layer::Main).shapes.hashes[0];
    let h1_b = h.ui.tree(Layer::Main).shapes.hashes[1];
    assert_eq!(h0_a, h1_a, "unchanged shape 0 must keep its hash");
    assert_ne!(h0_b, h1_b, "changed shape 1 must flip its hash");
}

/// Nesting reaches `cascade_static`, so re-parenting alone invalidates a
/// retained cascade.
///
/// Same three widget ids, same per-node configuration, same node count —
/// only the shape differs: two siblings under the root versus one nested
/// inside the other. Every per-node hash is therefore identical and the
/// count matches, so nothing *but* `subtree_end` distinguishes the two.
///
/// `CascadeEngine::can_update` used to catch this by zipping the whole
/// `subtree_ends` column against the tree on every run — an O(nodes) walk
/// per layer per frame, on the incremental fast path. Folding the end into
/// this hash covers the same ground for free, which is what lets
/// `LayerCascade::subtree_ends` be the sparse ancestry column its doc claims.
/// If the fold is ever dropped, these two collide and a re-parent silently
/// keeps the stale cascade.
#[test]
fn nesting_alone_changes_cascade_static() {
    let leaf = |ui: &mut Ui, name: &'static str| {
        Panel::hstack()
            .id(WidgetId::from_hash(name))
            .show(ui, |_| {})
            .response
            .node()
    };

    let siblings = record(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                leaf(ui, "a");
                leaf(ui, "b");
            })
            .response
            .node()
    })
    .cascade_static;
    let nested = record(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack().id(WidgetId::from_hash("a")).show(ui, |ui| {
                    leaf(ui, "b");
                });
            })
            .response
            .node()
    })
    .cascade_static;

    assert_ne!(
        siblings, nested,
        "re-parenting must invalidate the retained cascade; without \
         `subtree_end` in the fold these two hash the same",
    );
}

/// A paint animation is part of what a shape paints, so it moves the
/// node hash: adding one, dropping one, and changing what it animates
/// each read as a change, and the same animation twice reads the same.
#[test]
fn a_paint_animation_moves_the_node_hash() {
    use crate::scene::tree::paint_anims::paint_anim::PaintAnim;
    use std::time::Duration;

    fn build(ui: &mut Ui, anim: Option<PaintAnim>) -> NodeId {
        Panel::canvas()
            .id(WidgetId::from_hash("spinner"))
            .show(ui, |ui| {
                let line = Shape::line(
                    Vec2::new(0.0, 10.0),
                    Vec2::new(40.0, 10.0),
                    Stroke::new(RgbaF32::WHITE, 2.0),
                );
                match anim {
                    Some(anim) => ui.add_shape_animated(line, anim),
                    None => ui.add_shape(line),
                }
            })
            .response
            .node()
    }
    let turn = PaintAnim::turn(0.0, 1.0).with_period(Duration::from_secs(2));
    let still = record(|ui| build(ui, None)).node;
    let spun = record(|ui| build(ui, Some(turn))).node;
    let spun_again = record(|ui| build(ui, Some(turn))).node;
    let faster = record(|ui| build(ui, Some(turn.with_period(Duration::from_secs(1))))).node;
    let fading = record(|ui| build(ui, Some(PaintAnim::alpha(1.0, 0.0)))).node;
    assert_ne!(still, spun, "adding an animation");
    assert_eq!(spun, spun_again, "the same animation");
    assert_ne!(spun, faster, "a different period");
    assert_ne!(spun, fading, "a different channel");
}

/// Which half of the rollup each kind of edit moves. Paint-only edits
/// move the full subtree hash and leave the layout half, which the
/// measure cache keys on; layout edits move both, the full hash being
/// built from the layout half. A child's text is a layout input of the
/// child, so it reaches both of the parent's rollups through the child.
#[test]
fn each_edit_moves_the_half_it_belongs_to() {
    use crate::widgets::text::Text;

    #[derive(Clone, Copy, Debug)]
    struct Edit {
        fill: RgbaF32,
        label: &'static str,
        padding: f32,
    }
    let base = Edit {
        fill: RgbaF32::srgb(0.2, 0.4, 0.8),
        label: "hello",
        padding: 4.0,
    };
    let hashes = |edit: Edit| {
        let mut h = UiHarness::new(SURFACE);
        let node = h.frame_value(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .padding(edit.padding)
                .background(Background::fill(edit.fill))
                .show(ui, |ui| {
                    Text::new(edit.label)
                        .id(WidgetId::from_hash("label"))
                        .show(ui);
                })
                .response
                .node()
        });
        let rollups = &h.ui.tree(Layer::Main).rollups;
        (
            rollups.subtree[node.idx()],
            rollups.layout_subtree[node.idx()],
        )
    };
    let (full, layout) = hashes(base);
    let rows: [(&str, Edit, bool, bool); 3] = [
        (
            "recolour",
            Edit {
                fill: RgbaF32::srgb(0.9, 0.1, 0.1),
                ..base
            },
            true,
            false,
        ),
        (
            "padding",
            Edit {
                padding: 8.0,
                ..base
            },
            true,
            true,
        ),
        (
            "child text",
            Edit {
                label: "goodbye",
                ..base
            },
            true,
            true,
        ),
    ];
    for (label, edit, full_moves, layout_moves) in rows {
        let (f, l) = hashes(edit);
        assert_eq!(f != full, full_moves, "{label}: full subtree hash");
        assert_eq!(l != layout, layout_moves, "{label}: layout subtree hash");
    }
}
