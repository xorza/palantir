//! What a transform on a parent does to the damage under it.

use crate::damage::Damage;
use crate::damage::region::DamageRegion;
use crate::damage::tests::support::{BLUE, RED};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// When a transformed parent's child changes authoring, the damage rect covers
/// the child's screen rect (post-transform), not its layout rect, or the
/// scissor would clip the real paint position.
#[test]
fn child_under_transformed_parent_damage_in_screen_space() {
    let translate = Vec2::new(100.0, 0.0);
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let build = |fill: RgbaF32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(translate))
                .show(ui, |ui| {
                    *child = Some(
                        Block::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background::fill(fill))
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(RgbaF32::srgb(0.2, 0.4, 0.8), &mut h, &mut child_node);
    build(RgbaF32::srgb(0.9, 0.4, 0.8), &mut h, &mut child_node);

    // The child's layout rect is at (0, 0); its screen rect after the parent's
    // translate is (100, 0), where the GPU paints. Damage must cover that.
    let child_layout_rect = h.ui.arranged_rect(Layer::Main, child_node.unwrap());
    let expected_screen_rect = Rect {
        min: child_layout_rect.min + translate,
        size: child_layout_rect.size,
    };
    let region = h.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.min.x >= 100.0 - 0.5,
        "damage min.x must reflect parent translate; got {damage_rect:?}, expected near {expected_screen_rect:?}",
    );
    assert_eq!(damage_rect, expected_screen_rect);
}

/// Animating a parent's transform shifts every child's screen rect with no
/// authoring change. Damage must cover both prev and curr rects, or old pixels
/// streak through `LoadOp::Load`.
#[test]
fn animated_parent_transform_unions_old_and_new_positions() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let build = |dx: f32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Block::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut h, &mut child_node);
    build(50.0, &mut h, &mut child_node);

    // Layout unchanged; the transform shifted by (50, 0). Prev (0,0,40,40),
    // curr (50,0,40,40), gap 10 px: bbox 90×40 = 3600, sum = 3200, SAH cost
    // 400 is under the budget, so the merge collapses to one bbox (a far larger
    // distance stays split; see `transform_animation_keeps_far_positions_split`).
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(50.0, 0.0, 40.0, 40.0);
    assert_eq!(
        rects,
        vec![prev.union(curr)],
        "near transform animation → one merged bbox",
    );
    // The child is dirty: its screen rect moved. The parent is on the dirty
    // list too (its transform is in `node_hash`), but its changed-paints arm
    // emits nothing: its only row (the child marker) and `cascade_input` are
    // unchanged, so all damage comes from the child.
    let dirty_widget_ids: Vec<WidgetId> = h
        .engines
        .damage
        .counters
        .dirty()
        .iter()
        .map(|n| h.ui.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert_eq!(
        dirty_widget_ids,
        vec![WidgetId::from_hash("outer"), WidgetId::from_hash("c")],
    );
}

/// Under a tight pass-budget a far-apart transform animation keeps prev and
/// curr screen rects split; pinning both ends of the merge rule stops a budget
/// tweak flipping behaviour silently.
#[test]
fn transform_animation_keeps_far_positions_split() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let build = |dx: f32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Block::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut h, &mut child_node);
    build(200.0, &mut h, &mut child_node);

    // prev (0,0,40,40) area 1600; curr (200,0,40,40) area 1600. bbox 240×40 =
    // 9600, SAH cost 6400 would merge under the default 20 000 budget; a budget
    // of 0 pins the strict-overlap-only branch.
    let rects: Vec<Rect> = DamageRegion::collapse_from(
        &h.engines.damage.raw_rects,
        0.0,
        h.ui.display().logical_rect(),
    )
    .region
    .iter_rects()
    .collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(200.0, 0.0, 40.0, 40.0);
    assert_eq!(rects.len(), 2, "far transform animation → two rects");
    assert!(rects.contains(&prev) && rects.contains(&curr), "{rects:?}");
}

/// Soundness pin: when an ancestor's transform changes, a node whose own
/// `paint_rect` is clipped invariant (its direct shapes extend past the
/// viewport on every frame, so `clip_to(...)` saturates to the same rect)
/// must still contribute its `paint_rect` to damage, or the moved pixels'
/// old positions are never cleared.
///
/// Reproduces darkroom's "panning Scroll over a node-graph Canvas leaves
/// bezier trails": the canvas's beziers are direct shapes, its clipped paint
/// rect saturates, its `node_hash` is stable but `cascade_input` shifts each
/// pan frame.
#[test]
fn transform_shifted_direct_shape_with_invariant_clipped_paint_rect_contributes_damage() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let build = |dx: f32, h: &mut UiHarness| {
        h.frame(|ui| {
            // The outermost clip pins descendants to the surface; without it
            // `parent_clip = None` and the inner rect translates freely.
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                                .show(ui, |ui| {
                                    // Wider than the surface so the clipped
                                    // paint rect saturates and stays invariant
                                    // under small `dx`.
                                    ui.add_shape(
                                        Shape::rect(Rect::new(-200.0, 0.0, 500.0, 50.0))
                                            .fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                                    );
                                });
                        });
                });
        });
    };
    build(0.0, &mut h);
    build(5.0, &mut h);
    let region = h.damage_region();
    let covered = region.iter_rects().any(|r| {
        // Damage must cover the inner node's clipped paint area (0..100 ×
        // 0..50), where the shape's pixels live before and after the pan.
        r.min.x <= 0.5 && r.min.y <= 0.5 && r.max().x >= 50.0 - 0.5 && r.max().y >= 50.0 - 0.5
    });
    assert!(
        covered,
        "ancestor-transform shift moves a direct-shape leaf's pixels; \
         damage must still cover the shape area even though the \
         clipped paint_rect is invariant. region = {region:?}",
    );
}

/// Sister test: the "cascade_input shift on a direct-paint node → push
/// `curr_rect`" branch must not trip `FULL_REPAINT_THRESHOLD` for a pan of a
/// modestly sized clip-saturated node. Same setup, several pan ticks; each
/// step's damage stays `Partial` and bounded to the inner clipped area.
#[test]
fn pan_with_invariant_clipped_paint_rect_stays_partial() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let build = |dx: f32, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                                .show(ui, |ui| {
                                    ui.add_shape(
                                        Shape::rect(Rect::new(-200.0, 0.0, 500.0, 50.0))
                                            .fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                                    );
                                });
                        });
                });
        });
    };
    build(0.0, &mut h);
    for dx in [3.0, 6.0, 9.0, 12.0] {
        build(dx, &mut h);
        let collapsed = h.collapsed_damage();
        let damage = Damage::new(collapsed);
        assert!(
            matches!(damage, Some(Damage::Partial(_))),
            "pan with clip-saturated direct-paint node must stay Partial \
             (the new diff branch pushes one paint_rect per shifted node; \
             that must not blow past FULL_REPAINT_THRESHOLD on a single tick). \
             dx = {dx}, region = {:?}, damage = {damage:?}",
            collapsed.region,
        );
    }
}

/// Reproduces the darkroom graph-canvas regression: a panel with
/// `Panel::transform` and direct shapes shifts its own transform every pan
/// frame. Those shapes paint inside the self-transform so their pixels move,
/// but `cascade_input` tracks only ancestor state. The fix: own transform
/// folds into `node_hash`, so the diff's `e.get().hash == curr_node_hash`
/// guard fails and the Occupied arm pushes both prev and curr rects.
#[test]
fn self_transform_shift_damages_direct_shapes() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let build = |dx: f32, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id(WidgetId::from_hash("xpanel"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .show(ui, |ui| {
                            // Direct shape on the transformed panel, as darkroom's beziers.
                            ui.add_shape(
                                Shape::rect(Rect::new(40.0, 40.0, 30.0, 30.0))
                                    .fill(RgbaF32::srgb(0.2, 0.6, 0.9)),
                            );
                        });
                });
        });
    };
    build(0.0, &mut h);
    build(20.0, &mut h);
    let region = h.damage_region();

    // Translating self by dx=20 moves the shape's pixels from [40, 70] ×
    // [40, 70] to [60, 90] × [40, 70]. Damage covers at least [40, 90] × [40, 70].
    let covered = region.iter_rects().any(|r| {
        r.min.x <= 40.5 && r.min.y <= 40.5 && r.max().x >= 90.0 - 0.5 && r.max().y >= 70.0 - 0.5
    });
    assert!(
        covered,
        "self-transform shift on a panel with direct shapes must \
         damage both old and new shape positions. region = {region:?}",
    );
}

/// Pin the moved-subtree tier: a transformed parent over an
/// authoring-identical subtree damages exactly `prev extent ∪ curr extent`,
/// and the bulk snapshot refresh leaves next frame's baseline intact:
///
/// - a second tick's damage is anchored at the refreshed positions (a refresh
///   that forgot the rows' screens would still cover the original position);
/// - a still frame after the motion is a clean `Skip` (refreshed
///   `cascade_input` lets tier 1 skip at the subtree root).
#[test]
fn moved_subtree_damages_extents_and_refreshes_snapshots() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let build = |dx: f32, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .show(ui, |ui| {
                            for key in ["a", "b"] {
                                Block::new()
                                    .id(WidgetId::from_hash(key))
                                    .size(40.0)
                                    .background(Background::fill(BLUE))
                                    .show(ui);
                            }
                        });
                });
        });
    };

    build(0.0, &mut h);

    // Tick 1: dx 0 → 30. "outer"'s transform rides its node_hash, so it takes
    // the changed-paints arm (child marker matches, no damage); "inner"'s
    // authoring is untouched but its cascade prefix moved, so it takes the
    // moved-subtree tier. Extent = prev (0,0,80,40) and curr (30,0,80,40),
    // intersecting, merged into one bbox.
    build(30.0, &mut h);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 110.0, 40.0)],
        "tick 1: prev ∪ curr subtree extents",
    );

    // Tick 2: dx 30 → 60. Damage must anchor at the tick-1 position (left
    // edge 30, not 0), proving the refresh copied the rows' screens.
    build(60.0, &mut h);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 110.0, 40.0)],
        "tick 2: damage anchored at the refreshed (tick-1) extent",
    );

    // Still frame: identical dx, tier 1 skips at the root: no dirty nodes,
    // clean Skip. Fails if the bulk refresh corrupted a snapshot field.
    build(60.0, &mut h);
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "still frame after motion must not dirty any node",
    );
    assert_eq!(
        Damage::new(h.collapsed_damage()),
        None,
        "still frame after motion",
    );
}

/// A content change under a constant transform must not take the moved-subtree
/// tier (`subtree_hash` differs): the per-row diff gives leaf-tight damage.
#[test]
fn content_change_under_constant_transform_stays_row_tight() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let build = |fill: RgbaF32, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(30.0, 0.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash("a"))
                                .size(40.0)
                                .background(Background::fill(fill))
                                .show(ui);
                            Block::new()
                                .id(WidgetId::from_hash("b"))
                                .size(40.0)
                                .background(Background::fill(BLUE))
                                .show(ui);
                        });
                });
        });
    };
    build(BLUE, &mut h);
    build(RED, &mut h);
    // Only "a" changed: damage is its screen rect (layout 0..40 plus the 30 px
    // transform), not the inner extent, which would reach x = 110 and cover "b".
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 40.0, 40.0)],
        "fill flip under constant transform damages only the leaf",
    );
}
