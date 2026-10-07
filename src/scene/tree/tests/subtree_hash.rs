//! The rollup: what a subtree hash covers, and where its span ends.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::{SURFACE, record};
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};

#[test]
fn subtree_hash_stable_across_frames() {
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("b"))
                    .size(30.0)
                    .background(Background::fill(RgbaF32::srgb(0.9, 0.1, 0.1)))
                    .show(ui);
            })
            .response
            .node()
    };
    assert_eq!(record(build).subtree, record(build).subtree);
}

#[test]
fn subtree_hash_changes_when_descendant_changes() {
    fn build(ui: &mut Ui, fill: RgbaF32) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background::fill(fill))
                    .show(ui);
            })
            .response
            .node()
    }
    let h1 = record(|ui| build(ui, RgbaF32::srgb(0.2, 0.4, 0.8))).subtree;
    let h2 = record(|ui| build(ui, RgbaF32::srgb(0.9, 0.4, 0.8))).subtree;
    assert_ne!(h1, h2, "leaf change must invalidate every ancestor");
}

#[test]
fn subtree_hash_changes_on_sibling_reorder() {
    fn build(ui: &mut Ui, swap: bool) -> NodeId {
        let a = |ui: &mut Ui| {
            Block::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                .show(ui);
        };
        let b = |ui: &mut Ui| {
            Block::new()
                .id(WidgetId::from_hash("b"))
                .size(30.0)
                .background(Background::fill(RgbaF32::srgb(0.9, 0.1, 0.1)))
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
    let h_ab = record(|ui| build(ui, false)).subtree;
    let h_ba = record(|ui| build(ui, true)).subtree;
    assert_ne!(h_ab, h_ba);
}

/// A panel's own `Panel::transform` changing flips both `node_hash` and `subtree_hash`. The `node_hash` change is load-bearing: the transform applies to the panel's direct shapes, and `DamageEngine::compute` keys self-paint damage off `node_hash`.
#[test]
fn self_transform_change_flips_node_hash() {
    use crate::primitives::geometry::translate_scale::TranslateScale;
    use glam::Vec2;
    fn build(ui: &mut Ui, t: TranslateScale) -> NodeId {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .transform(t)
            .show(ui, |_| {})
            .response
            .node()
    }
    // Both transforms are non-identity: identity is the noop sentinel (`PanelExtras::DEFAULT.transform`) and would carry no row.
    let t_a = TranslateScale::from_translation(Vec2::new(1.0, 0.0));
    let t_b = TranslateScale::from_translation(Vec2::new(10.0, 0.0));
    let (a, b) = (record(|ui| build(ui, t_a)), record(|ui| build(ui, t_b)));
    assert_ne!(a.node, b.node, "self transform MUST change node hash");
    assert_ne!(
        a.subtree, b.subtree,
        "self transform MUST change subtree hash"
    );
    // A transform moves no rect, so neither the measure cache nor cascade structural tables may see it.
    assert_eq!(
        a.layout_subtree, b.layout_subtree,
        "self transform must not change the layout half"
    );
    assert_eq!(
        a.cascade_static, b.cascade_static,
        "self transform must not change the cascade-static hash"
    );

    let identity = TranslateScale::IDENTITY;
    let visual_noop = TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5);
    assert_eq!(
        record(|ui| build(ui, identity)).node,
        record(|ui| build(ui, visual_noop)).node,
    );
}

/// `LayoutMode::Grid(idx)` carries a frame-local arena slot; the node hash must depend only on def contents (rolled in at `NodeExit`), so the same grid in different positions hashes alike.
#[test]
fn grid_per_node_hash_independent_of_arena_slot() {
    use crate::primitives::layout::track::Track;
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
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(10.0)
                    .show(ui);
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        Block::new()
                            .id(WidgetId::from_hash("b"))
                            .size(10.0)
                            .show(ui);
                        Block::new()
                            .id(WidgetId::from_hash("c"))
                            .size(10.0)
                            .show(ui);
                    });
                Block::new()
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
            Block::new()
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

/// The `subtree_hash` rollup is root-local: a second top-level subtree recorded back-to-back hashes independently of the first.
#[test]
fn subtree_hash_rollup_root_local_across_two_roots() {
    fn build(ui: &mut Ui, root_a_color: RgbaF32) -> u32 {
        Panel::vstack()
            .id(WidgetId::from_hash("root-a"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a-leaf"))
                    .size(50.0)
                    .background(Background::fill(root_a_color))
                    .show(ui);
            });
        let b_first = ui.tree(Layer::Main).records.len() as u32;
        Panel::vstack()
            .id(WidgetId::from_hash("root-b"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("b-leaf"))
                    .size(30.0)
                    .show(ui);
            });
        b_first
    }
    let mut ui1 = UiHarness::new(SURFACE);
    let mut b_first1 = 0;
    ui1.frame(|ui| {
        b_first1 = build(ui, RgbaF32::srgb(1.0, 0.0, 0.0));
    });
    let h_b1 = ui1.ui.tree(Layer::Main).rollups.subtree[b_first1 as usize];

    let mut ui2 = UiHarness::new(SURFACE);
    let mut b_first2 = 0;
    ui2.frame(|ui| {
        b_first2 = build(ui, RgbaF32::srgb(0.0, 1.0, 0.0));
    });
    let h_b2 = ui2.ui.tree(Layer::Main).rollups.subtree[b_first2 as usize];
    assert_eq!(b_first1, b_first2);
    assert_eq!(h_b1, h_b2, "root B's subtree_hash must not fold root A");
}
