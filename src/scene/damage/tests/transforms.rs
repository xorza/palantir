//! What a transform on a parent does to the damage under it.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, translate_scale::TranslateScale};
use crate::scene::damage::Damage;
use crate::scene::damage::tests::support::{BLUE, RED};
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::record::NodeId;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin: when a transformed parent's child changes authoring, the
/// damage rect covers the child's *screen* rect (post-transform),
/// not its layout rect. Without this, the backend scissor would
/// clip the actual paint position and leave the screen unchanged.
#[test]
fn child_under_transformed_parent_damage_in_screen_space() {
    let translate = Vec2::new(100.0, 0.0);
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let build = |fill: Color, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(translate))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: fill.into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut h, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut h, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = h.ui.layout[Layer::Main].rect[child_node.unwrap().idx()];
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

/// Pin: animating a parent's transform shifts every child's screen
/// rect even though the children's authoring is unchanged. The
/// damage union must cover both prev and curr screen rects so the
/// backend repaints over the old positions too (otherwise the old
/// frame's pixels would streak through `LoadOp::Load`).
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
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut h, &mut child_node);
    build(50.0, &mut h, &mut child_node);

    // Child layout rect didn't change. Parent's transform shifted by
    // (50, 0). Prev screen rect = (0,0,40,40); curr = (50,0,40,40);
    // gap of 10 px between them. bbox = 90×40 = 3600, sum = 3200,
    // SAH cost = 400 ≪ default budget — the merge rule collapses
    // into one bbox. (A *much* larger distance would push cost over
    // the budget; pinned by
    // `transform_animation_keeps_far_positions_split`.)
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(50.0, 0.0, 40.0, 40.0);
    assert_eq!(
        rects,
        vec![prev.union(curr)],
        "near transform animation → one merged bbox",
    );
    // The child is dirty: its authoring is unchanged but its screen
    // rect moved (rect comparison catches this). The parent lands on
    // the dirty list too — its self-transform is part of `node_hash`
    // (panel extras), so the changed transform routes it to the
    // changed-paints arm — but that arm emits nothing for it: its
    // only row (the child marker) is unchanged and its own
    // `cascade_input` is stable, so all damage comes from the child.
    let dirty_widget_ids: Vec<WidgetId> =
        h.ui.damage_engine
            .counters
            .dirty()
            .iter()
            .map(|n| h.ui.forest.trees[Layer::Main].records.widget_id()[n.idx()])
            .collect();
    assert_eq!(
        dirty_widget_ids,
        vec![WidgetId::from_hash("outer"), WidgetId::from_hash("c")],
    );
}

/// Sister case to the test above: under a tight pass-budget, a
/// far-apart transform animation keeps prev and curr screen rects
/// split. Pinning both ends of the merge rule means a budget tweak
/// can't silently flip behaviour without breaking a test.
#[test]
fn transform_animation_keeps_far_positions_split() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    // Drop the merge budget to strict-overlap-only so the prev/curr
    // pair (cost 6 400 < default budget) stays split. Pins both
    // ends of the merge rule against future budget tweaks.
    h.ui.damage_engine.budget_px = 0.0;
    let mut child_node = None;
    let build = |dx: f32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut h, &mut child_node);
    build(200.0, &mut h, &mut child_node);

    // prev (0,0,40,40) area 1600; curr (200,0,40,40) area 1600.
    // bbox 240×40 = 9600. SAH cost = 6400 — under the default
    // 20 000 budget, this would merge; the guard above drops the
    // budget to 0 to pin the strict-overlap-only branch.
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(200.0, 0.0, 40.0, 40.0);
    assert_eq!(rects.len(), 2, "far transform animation → two rects");
    assert!(rects.contains(&prev) && rects.contains(&curr), "{rects:?}");
}

/// Soundness pin: when an ancestor's transform changes, a node whose
/// own `paint_rect` is **clipped invariant** (because its direct
/// shapes extend past the viewport / clip on every frame, so
/// `clip_to(...)` saturates to the same rect both passes) must still
/// contribute its `paint_rect` to damage. Otherwise the pixels of
/// those shapes — which DID move with the parent transform — get
/// stranded; the old positions never get cleared.
///
/// Repro of darkroom's "panning Scroll over a node-graph Canvas
/// leaves bezier trails": canvas's connection beziers are direct
/// shapes; canvas is wider than the viewport so its clipped paint
/// rect saturates; canvas's `node_hash` is stable but its
/// `cascade_input` shifts every pan frame.
#[test]
fn transform_shifted_direct_shape_with_invariant_clipped_paint_rect_contributes_damage() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let build = |dx: f32, h: &mut UiHarness| {
        h.frame(|ui| {
            // Outermost clip pins descendants to the surface viewport
            // — without it, `parent_clip = None` and inner's paint
            // rect translates freely (the bug then doesn't manifest;
            // damage catches the rect change via the normal path).
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
                                    // Shape wider than the surface so
                                    // the clipped paint rect
                                    // saturates and stays invariant
                                    // under small `dx` translates.
                                    ui.add_shape(
                                        Shape::rect(Rect::new(-200.0, 0.0, 500.0, 50.0))
                                            .fill(Color::rgb(1.0, 0.0, 0.0)),
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
        // Damage must cover the inner node's clipped paint area
        // (0..100 × 0..50) — that's where the shape's pixels live
        // both before and after the small pan.
        r.min.x <= 0.5 && r.min.y <= 0.5 && r.max().x >= 50.0 - 0.5 && r.max().y >= 50.0 - 0.5
    });
    assert!(
        covered,
        "ancestor-transform shift moves a direct-shape leaf's pixels; \
         damage must still cover the shape area even though the \
         clipped paint_rect is invariant. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Sister test to the soundness pin above: the new "cascade_input
/// shift on a direct-paint node → push `curr_rect`" branch in the
/// damage diff must not trip `FULL_REPAINT_THRESHOLD` for a pan of a
/// modestly-sized clip-saturated node. Same setup as that pin, but
/// repeated for several pan ticks; each step's damage stays
/// `Partial` and stays bounded to the inner clipped area.
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
                                            .fill(Color::rgb(1.0, 0.0, 0.0)),
                                    );
                                });
                        });
                });
        });
    };
    build(0.0, &mut h);
    for dx in [3.0, 6.0, 9.0, 12.0] {
        build(dx, &mut h);
        let region = h.damage_region();
        let damage = Damage::new(region);
        assert!(
            matches!(damage, Damage::Partial(_)),
            "pan with clip-saturated direct-paint node must stay Partial \
             (the new diff branch pushes one paint_rect per shifted node; \
             that must not blow past FULL_REPAINT_THRESHOLD on a single tick). \
             dx = {dx}, region = {:?}, damage = {damage:?}",
            region.iter_rects().collect::<Vec<_>>(),
        );
    }
}

/// Reproduces the darkroom graph-canvas regression: a panel with
/// `Panel::transform` and direct shapes (bezier connections) shifts
/// its own transform every pan frame. Under the `Panel::transform`
/// contract those shapes paint *inside* the self-transform, so their
/// painted pixels move — but `cascade_input` only tracks
/// ancestor state and stays put. The fix is at the source: own
/// transform now folds into `node_hash`, so the diff's
/// `e.get().hash == curr_node_hash` guard fails and the generic
/// Occupied arm pushes both prev and curr rects, sweeping where the
/// shapes were and are.
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
                            // Direct shape on the transformed panel —
                            // mirrors how darkroom adds connection
                            // beziers on the inner canvas.
                            ui.add_shape(
                                Shape::rect(Rect::new(40.0, 40.0, 30.0, 30.0))
                                    .fill(Color::rgb(0.2, 0.6, 0.9)),
                            );
                        });
                });
        });
    };
    build(0.0, &mut h);
    build(20.0, &mut h);
    let region = h.damage_region();

    // After translating self by dx=20, the shape's prev pixels lived
    // at [40, 70] × [40, 70] (translation 0) and the new pixels live
    // at [60, 90] × [40, 70]. Damage must cover both — i.e. at least
    // [40, 90] × [40, 70].
    let covered = region.iter_rects().any(|r| {
        r.min.x <= 40.5 && r.min.y <= 40.5 && r.max().x >= 90.0 - 0.5 && r.max().y >= 70.0 - 0.5
    });
    assert!(
        covered,
        "self-transform shift on a panel with direct shapes must \
         damage both old and new shape positions. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin the moved-subtree tier (tier 1.5): a transformed parent over an
/// authoring-identical subtree damages exactly `prev extent ∪ curr
/// extent`, and — the load-bearing part — the bulk snapshot refresh
/// leaves next frame's baseline intact:
///
/// - a second tick's damage is anchored at the *refreshed* positions
///   (if the refresh forgot to copy the rows' screens, damage would
///   still cover the original position);
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
                                Frame::new()
                                    .id(WidgetId::from_hash(key))
                                    .size(40.0)
                                    .background(Background {
                                        fill: BLUE.into(),
                                        ..Default::default()
                                    })
                                    .show(ui);
                            }
                        });
                });
        });
    };

    build(0.0, &mut h);

    // Tick 1: dx 0 → 30. "outer"'s own transform rides its node_hash
    // (panel extras), so outer takes the changed-paints arm (child
    // marker matches exactly — no damage); "inner"'s authoring is
    // untouched but its cascade prefix moved → tier 1.5. Subtree
    // extent = both 40×40 frames side by side: prev (0,0,80,40),
    // curr (30,0,80,40) — intersecting, so the region merges them
    // into one bbox.
    build(30.0, &mut h);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 110.0, 40.0)],
        "tick 1: prev ∪ curr subtree extents",
    );

    // Tick 2: dx 30 → 60. Damage must anchor at the tick-1 position —
    // its left edge is 30, not 0 — proving the tier refreshed the
    // rows' screens, not just `cascade_input`.
    build(60.0, &mut h);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 110.0, 40.0)],
        "tick 2: damage anchored at the refreshed (tick-1) extent",
    );

    // Still frame: identical dx → tier 1 skips at the root, no dirty
    // nodes, clean Skip. Fails loudly if the bulk refresh corrupted
    // any snapshot field.
    build(60.0, &mut h);
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "still frame after motion must not dirty any node",
    );
    assert_eq!(
        Damage::new(h.damage_region()),
        Damage::Skip,
        "still frame after motion",
    );
}

/// Sister pin: a *content* change under a constant transform must not
/// take the moved-subtree tier (`subtree_hash` differs) — the per-row
/// diff still produces leaf-tight damage, not the subtree extent.
#[test]
fn content_change_under_constant_transform_stays_row_tight() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let build = |fill: Color, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(30.0, 0.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("a"))
                                .size(40.0)
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                            Frame::new()
                                .id(WidgetId::from_hash("b"))
                                .size(40.0)
                                .background(Background {
                                    fill: BLUE.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        });
                });
        });
    };
    build(BLUE, &mut h);
    build(RED, &mut h);
    // Only "a" changed; damage is its screen rect (layout 0..40 + the
    // 30 px transform), NOT the whole inner extent (which would reach
    // x = 110 and cover the untouched "b").
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 40.0, 40.0)],
        "fill flip under constant transform damages only the leaf",
    );
}
