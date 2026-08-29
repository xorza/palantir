//! The rollup: what a subtree hash covers, and where its span ends.

use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::{SURFACE, record_cascade_static, record_hash};
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};

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
            .response
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
            .response
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
            .response
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
    use crate::primitives::translate_scale::TranslateScale;
    use glam::Vec2;
    fn build(ui: &mut Ui, t: TranslateScale) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .transform(t)
            .show(ui, |_| {})
            .response
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

    let cols = [Track::FILL, Track::FILL];
    let rows = [Track::FILL];

    let mut ui1 = UiHarness::new(SURFACE);
    let mut g1 = None;
    ui1.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                g1 = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("target"))
                        .cols(cols)
                        .rows(rows)
                        .show(ui, |_| {})
                        .response
                        .node(),
                );
                Grid::new()
                    .id(WidgetId::from_hash("other"))
                    .cols(cols)
                    .rows(rows)
                    .show(ui, |_| {});
            });
    });
    let mut ui2 = UiHarness::new(SURFACE);
    let mut g2 = None;
    ui2.frame(|ui| {
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
                        .response
                        .node(),
                );
            });
    });
    assert_eq!(
        ui1.ui.tree(Layer::Main).rollups.node[g1.unwrap().idx()],
        ui2.ui.tree(Layer::Main).rollups.node[g2.unwrap().idx()],
    );
}

#[test]
fn subtree_end_rolls_up_during_recording() {
    let mut h = UiHarness::new(SURFACE);
    let root = h.frame_value(|ui| {
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
            .response
            .node()
    });
    // Pre-order: 0=viewport 1=root 2=a 3=inner 4=b 5=c 6=d
    assert_eq!(h.ui.tree(Layer::Main).records.len(), 7);
    let ends = h.ui.tree(Layer::Main).records.subtree_end();
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
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| nest(ui, 16));
    let n = h.ui.tree(Layer::Main).records.len() as u32;
    // Synthetic viewport + 16 nested vstacks + 1 leaf frame.
    assert_eq!(n, 18);
    for i in 0..(n - 1) {
        assert_eq!(
            h.ui.tree(Layer::Main).records.subtree_end()[i as usize].end(),
            n,
            "every ancestor on the chain points past the leaf",
        );
    }
    assert_eq!(
        h.ui.tree(Layer::Main).records.subtree_end()[(n - 1) as usize].end(),
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
        let b_first = ui.tree(Layer::Main).records.len() as u32;
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
    let mut ui1 = UiHarness::new(SURFACE);
    let mut b_first1 = 0;
    ui1.frame(|ui| {
        b_first1 = build(ui, Color::rgb(1.0, 0.0, 0.0));
    });
    let h_b1 = ui1.ui.tree(Layer::Main).rollups.subtree[b_first1 as usize];

    let mut ui2 = UiHarness::new(SURFACE);
    let mut b_first2 = 0;
    ui2.frame(|ui| {
        b_first2 = build(ui, Color::rgb(0.0, 1.0, 0.0));
    });
    let h_b2 = ui2.ui.tree(Layer::Main).rollups.subtree[b_first2 as usize];
    assert_eq!(b_first1, b_first2);
    assert_eq!(h_b1, h_b2, "root B's subtree_hash must not fold root A");
}

fn record_subtree_hash<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut h = UiHarness::new(SURFACE);
    let target = h.frame_value(|ui| f(ui));
    h.ui.tree(Layer::Main).rollups.subtree[target.idx()]
}
