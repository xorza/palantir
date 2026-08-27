//! What moves a single node's hash, and what deliberately does not.

use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::layout::types::{justify::Justify, sizing::Sizing};
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::{SURFACE, record_cascade_static, record_hash};
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
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
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .response
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
                ui.add_shape(Shape::polyline(points, PolylineColors::Single(color), 2.0));
            })
            .response
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
            .response
            .node()
    });
    let h2 = record_hash(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("b"))
            .show(ui, |_| {})
            .response
            .node()
    });
    assert_eq!(h1, h2);

    let static_1 = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("a"))
            .show(ui, |_| {})
            .response
            .node()
    });
    let static_2 = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("b"))
            .show(ui, |_| {})
            .response
            .node()
    });
    assert_ne!(
        static_1, static_2,
        "identity changes must rebuild cascade hit IDs and its by-id snapshot",
    );
}

#[test]
fn changing_layout_property_changes_hash() {
    use crate::scene::visibility::Visibility;
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
            .response
            .node()
    }
    let h1 = record_hash(|ui| build(ui, Color::rgb(0.2, 0.4, 0.8)));
    let h2 = record_hash(|ui| build(ui, Color::rgb(0.9, 0.4, 0.8)));
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
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::line(glam::Vec2::new(0.0, 0.0), glam::Vec2::new(10.0, 10.0), 1.0)
                        .brush(Color::rgb(1.0, 0.0, 0.0)),
                );
                ui.add_shape(
                    Shape::line(
                        glam::Vec2::new(10.0, 10.0),
                        glam::Vec2::new(20.0, 20.0),
                        1.0,
                    )
                    .brush(Color::rgb(0.0, 1.0, 0.0)),
                );
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
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::line(glam::Vec2::new(0.0, 0.0), glam::Vec2::new(10.0, 10.0), 1.0)
                        .brush(Color::rgb(1.0, 0.0, 0.0)),
                );
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
    let build = |b_endpoint: glam::Vec2, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::line(glam::Vec2::new(0.0, 0.0), glam::Vec2::new(10.0, 10.0), 1.0)
                        .brush(Color::rgb(1.0, 0.0, 0.0)),
                );
                ui.add_shape(
                    Shape::line(glam::Vec2::new(5.0, 5.0), b_endpoint, 1.0)
                        .brush(Color::rgb(0.0, 1.0, 0.0)),
                );
            });
    };
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(glam::Vec2::new(20.0, 20.0), ui));
    let h0_a = h.ui.tree(Layer::Main).shapes.hashes[0];
    let h0_b = h.ui.tree(Layer::Main).shapes.hashes[1];
    h.frame(|ui| build(glam::Vec2::new(30.0, 30.0), ui));
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
/// `Cascade::subtree_ends` be the sparse ancestry column its doc claims.
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

    let siblings = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                leaf(ui, "a");
                leaf(ui, "b");
            })
            .response
            .node()
    });
    let nested = record_cascade_static(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack().id(WidgetId::from_hash("a")).show(ui, |ui| {
                    leaf(ui, "b");
                });
            })
            .response
            .node()
    });

    assert_ne!(
        siblings, nested,
        "re-parenting must invalidate the retained cascade; without \
         `subtree_end` in the fold these two hash the same",
    );
}
