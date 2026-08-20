//! What one node redrawing damages, shape by shape.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, size::Size};
use crate::scene::damage::Damage;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, frame, one_frame};
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::text::TEXT_SCALE_STEP;
use crate::text::glyph_font::GlyphFont;
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin: the very first frame has no `prev_frame` entries, so every
/// painting node is "added" → marked dirty and contributes its rect.
/// The root Panel records no chrome and no direct shapes, so it's
/// non-painting and stays out of `dirty`/`region`.
#[test]
fn first_frame_marks_every_painting_node_dirty() {
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    let painting = h.ui.cascade.layers[Layer::Main]
        .paint_arena
        .node_spans
        .iter()
        .filter(|s| s.len > 0)
        .count();
    assert_eq!(h.ui.damage_engine.counters.dirty().len(), painting);
    // First frame is `force_full`, so `compute` short-circuits to
    // `Damage::Full` after the structural diff — and the Vacant arm
    // skips its raw-rect pushes (the region would be discarded), so
    // the buffer stays empty and its retained capacity never balloons
    // to whole-tree size on the first frame or a resize storm.
    assert!(h.ui.damage_engine.raw_rects.is_empty());
}

/// Pin: re-recording identical authoring → zero dirty nodes,
/// damage rect is `None`. The steady-state ideal: idle UI does
/// nothing.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut h, build);
    frame(&mut h, build);

    assert!(h.ui.damage_engine.counters.dirty().is_empty());
    assert!(h.damage_region().rects.is_empty());
    assert_eq!(Damage::new(h.collapsed_damage()), Damage::Skip,);
}

/// Pin: an authoring change on one leaf marks just that leaf
/// dirty; the parent (whose own fields didn't change and whose
/// rect is identical) stays clean.
#[test]
fn fill_change_marks_only_the_changed_leaf() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut h, |ui| {
        one_frame(ui, RED);
    });

    assert_eq!(h.ui.damage_engine.counters.dirty().len(), 1);
    let dirty_id = h.ui.damage_engine.counters.dirty()[0];
    assert_eq!(
        h.ui.forest.trees[Layer::Main].records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("a")
    );
    // DamageEngine rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        h.damage_region().iter_rects().next(),
        Some(h.ui.layout[Layer::Main].rect[dirty_id.idx()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::fixed(a_size), Sizing::fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::fixed(30.0), Sizing::fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    frame(&mut h, |ui| build(50.0, ui));
    frame(&mut h, |ui| build(80.0, ui));

    // `a` changed authoring (size). `b`'s authoring is unchanged
    // but its arranged x shifts from 50 → 80. Both are dirty.
    let dirty_ids: Vec<WidgetId> =
        h.ui.damage_engine
            .counters
            .dirty()
            .iter()
            .map(|n| h.ui.forest.trees[Layer::Main].records.widget_id()[n.idx()])
            .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("a")));
    assert!(dirty_ids.contains(&WidgetId::from_hash("b")));
}

/// Pin: a widget that disappears between frames contributes its
/// previous rect to damage — the renderer must repaint that
/// region to erase the leftover pixels.
#[test]
fn removed_widget_contributes_prev_rect_to_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    let prev_button_rect =
        h.ui.damage_engine
            .prev_paint_rect(WidgetId::from_hash("gone"))
            .expect("gone painted last frame");

    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Button is gone; root Panel is non-painting (no chrome) so it
    // never entered prev. Only contribution is the Button's prev
    // rect, surfaced via the `removed` list.
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, vec![prev_button_rect]);
}

/// Pin: an added widget that wasn't in last frame contributes
/// its current rect to damage and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("new"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });

    let dirty_ids: Vec<WidgetId> =
        h.ui.damage_engine
            .counters
            .dirty()
            .iter()
            .map(|n| h.ui.forest.trees[Layer::Main].records.widget_id()[n.idx()])
            .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(!h.damage_region().rects.is_empty());
}

/// Pin (motivating workload): hovering a button causes exactly one
/// node — the button — to be dirty, with damage rect == button's
/// rect. This is the bread-and-butter case Stage 3 is designed for:
/// pointer hover changes a small region; partial repaint should win.
///
/// Hit-test response lags by one frame (recording reads last frame's
/// state), so we run enough frames at each pointer position to let
/// the damage stream settle, then assert on the *transition* frame.
#[test]
fn button_hover_damage_covers_only_the_button() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |h: &mut UiHarness, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    // Pointer parked off-button. Settle for two frames so hit-test +
    // damage are at steady state (no diff).
    h.move_to(Vec2::new(380.0, 380.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "off-button pointer should reach a no-diff steady state"
    );

    let hot_rect = h.ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    let target = hot_rect.min + Vec2::new(5.0, 5.0);

    // Move pointer onto the hot button. The *next* post_record computes
    // hover=true. The frame *after* that records the button as
    // hovered → its fill differs → it lands in the dirty set alone.
    // `on_input` recomputes hover against the existing hit_index
    // immediately, so the *next* recording sees `hovered=true` and
    // emits the hovered fill. DamageEngine = button rect only.
    h.move_to(target);
    build(&mut h, &mut hot_node, &mut cold_node);

    assert_eq!(
        h.ui.damage_engine.counters.dirty().len(),
        1,
        "only the hovered button should be dirty"
    );
    let dirty_id = h.ui.damage_engine.counters.dirty()[0];
    assert_eq!(
        h.ui.forest.trees[Layer::Main].records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(h.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(h.collapsed_damage()).expect_partial(),
        hot_rect.into(),
        "small per-button damage must not trip the full-repaint heuristic",
    );

    // Next frame at same cursor → no diff (settled).
    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "settled hover should produce no further damage"
    );
}

/// Pin: leaving the button (un-hover) is symmetric — the only diff
/// is the button's fill flipping back, damage = button rect.
#[test]
fn button_unhover_damage_covers_only_the_button() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |h: &mut UiHarness, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    // Settle two frames with cursor over the hot button.
    build(&mut h, &mut hot_node, &mut cold_node);
    let hot_rect = h.ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    h.move_to(hot_rect.min + Vec2::new(5.0, 5.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "settled hover"
    );

    // Pointer leaves the button.
    h.move_to(Vec2::new(380.0, 380.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    assert_eq!(h.ui.damage_engine.counters.dirty().len(), 1);
    assert_eq!(
        h.ui.forest.trees[Layer::Main].records.widget_id()
            [h.ui.damage_engine.counters.dirty()[0].idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(h.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(h.collapsed_damage()).expect_partial(),
        hot_rect.into()
    );
}

/// `NodeSnapshot.paint_span` covers one entry per Paint row on the
/// node — chrome at row 0 when present, then each direct shape — with
/// matching rect and canonical hash. Mirrors `Cascade::paint_arenas`.
#[test]
fn node_snapshot_decomposition_matches_cascade() {
    use crate::Shape;
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), 1.0)
                        .brush(Color::rgb(1.0, 0.0, 0.0)),
                );
                ui.add_shape(
                    Shape::line(Vec2::new(20.0, 20.0), Vec2::new(30.0, 30.0), 1.0)
                        .brush(Color::rgb(0.0, 1.0, 0.0)),
                );
            });
    });

    let snap = h.ui.damage_engine.prev[&WidgetId::from_hash("multi")];
    let layer = Layer::Main;
    let node_idx = h.ui.cascade.by_id[&WidgetId::from_hash("multi")].node.idx();
    let node_span = h.ui.cascade.layers[layer].paint_arena.node_spans[node_idx];
    let layer_paints = &h.ui.cascade.layers[layer].paint_arena.rows;

    // Chrome lands at row 0 of the node's paint span when present.
    let chrome_paint = layer_paints[node_span.start as usize];
    assert!(
        chrome_paint.screen.area() > 0.0,
        "chrome panel must have non-zero chrome rect",
    );

    // Snapshot mirrors the cascade arena slice.
    let snap_paints = &h.ui.damage_engine.paints.slots[snap.paint_span.range()];
    assert_eq!(snap_paints.len(), 3, "chrome + 2 direct shapes ⇒ 3 rows");
    let cascade_paints = &layer_paints[node_span.range()];
    for (ord, p) in snap_paints.iter().enumerate() {
        assert_eq!(
            p.screen, cascade_paints[ord].screen,
            "paint #{ord} rect must match cascade column",
        );
        assert_eq!(
            p.hash, cascade_paints[ord].hash,
            "paint #{ord} hash must match cascade column",
        );
    }

    // The force-full first frame skips the Vacant pushes (its region
    // is discarded) — the buffer stays empty.
    assert!(h.ui.damage_engine.raw_rects.is_empty());

    // A widget added on an *incremental* frame hits the same Vacant
    // arm with the pushes live: one rect per paint row (chrome + each
    // shape). The unchanged "multi" subtree-skips and contributes
    // nothing, so the buffer holds exactly the newcomer's rows.
    let two_lines = |ui: &mut Ui| {
        ui.add_shape(
            Shape::line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), 1.0)
                .brush(Color::rgb(1.0, 0.0, 0.0)),
        );
        ui.add_shape(
            Shape::line(Vec2::new(20.0, 20.0), Vec2::new(30.0, 30.0), 1.0)
                .brush(Color::rgb(0.0, 1.0, 0.0)),
        );
    };
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| two_lines(ui));
        Panel::hstack()
            .id(WidgetId::from_hash("multi2"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| two_lines(ui));
    });
    let snap2 = h.ui.damage_engine.prev[&WidgetId::from_hash("multi2")];
    let snap2_paints = &h.ui.damage_engine.paints.slots[snap2.paint_span.range()];
    assert_eq!(
        h.ui.damage_engine.raw_rects.len(),
        3,
        "incremental Vacant insert pushes one rect per paint row",
    );
    assert_eq!(h.ui.damage_engine.raw_rects[0], snap2_paints[0].screen);
    assert_eq!(h.ui.damage_engine.raw_rects[1], snap2_paints[1].screen);
    assert_eq!(h.ui.damage_engine.raw_rects[2], snap2_paints[2].screen);
}

/// Slice 4 headline: a multi-shape owner whose shapes are spatially
/// disjoint pushes only the *changed* shape's rect pair on a frame
/// where one endpoint moved. Reproduces the darkroom graph pattern
/// (canvas owns N bezier connections; drag one node, only the
/// connections actually touching it should enter damage). Pre-slice-4
/// the Occupied-changed arm pushed `prev_rect ∪ curr_rect = union of
/// all shapes`; slice 4 pushes only the moved shape's prev + curr.
#[test]
fn per_shape_damage_only_pushes_changed_shapes() {
    use crate::Shape;

    // Two stable shapes (drawn at fixed coords) + one shape whose
    // endpoint shifts between frames. Frame N records all three;
    // frame N+1 shifts only the third — the diff must push exactly
    // that shape's pair of rects.
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |moving_y: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(180.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, 0.0, 20.0, 10.0)).fill(Color::rgb(1.0, 0.0, 0.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(60.0, 0.0, 20.0, 10.0)).fill(Color::rgb(0.0, 1.0, 0.0)),
                );
                // The moving shape, far from the other two, so its
                // bbox doesn't merge with theirs in the damage region.
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, moving_y, 20.0, 10.0))
                        .fill(Color::rgb(0.0, 0.0, 1.0)),
                );
            });
    };

    // Frame 1 (cold) and frame 2 (steady — no diff).
    frame(&mut h, |ui| build(120.0, ui));
    frame(&mut h, |ui| build(120.0, ui));
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "steady frame must produce no diff"
    );

    // Frame 3 nudges shape 2's y endpoint. Slice 4 contract: only
    // shape 2's prev rect (at y=120) and curr rect (at y=140) enter
    // the damage region. Chrome (canvas background) is unchanged in
    // geometry AND authoring → no chrome push. Shapes 0 and 1 are
    // bit-identical → no push.
    let prev_snap = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let prev_arena_len = h.ui.damage_engine.paints.slots.len();
    // paint_snaps row 0 is chrome; shapes follow at offset 1.
    let prev_shape2_rect =
        h.ui.damage_engine.paints.slots[prev_snap.paint_span.range()][1 + 2].screen;
    frame(&mut h, |ui| build(140.0, ui));

    let canvas_snap = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let curr_shape2_rect =
        h.ui.damage_engine.paints.slots[canvas_snap.paint_span.range()][1 + 2].screen;
    assert_eq!(
        canvas_snap.paint_span, prev_snap.paint_span,
        "same-count paint changes must refresh the existing arena span",
    );
    assert_eq!(
        h.ui.damage_engine.paints.slots.len(),
        prev_arena_len,
        "an in-place refresh must not touch the allocator at all",
    );

    // The damage region must intersect both old and new positions of
    // shape 2 (so the pixels-at-old-position get cleared and
    // pixels-at-new-position get painted). It must NOT intersect the
    // disjoint regions occupied by shapes 0 and 1 — those didn't move.
    let region = h.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));
    assert!(
        intersects(prev_shape2_rect),
        "old position of moved shape must be in damage region; \
         prev_rect = {prev_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        intersects(curr_shape2_rect),
        "new position of moved shape must be in damage region; \
         curr_rect = {curr_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Sentinel: a rect on the chrome's top edge between shapes 0/1
    // (y < 120) must NOT be in the region — chrome didn't change,
    // shapes 0/1 are unchanged, only the moving shape's y-band gets
    // damaged. Pre-slice-4 the whole `paint_rect` union (covering
    // the entire 180×180 canvas) would have hit. This is the
    // tight-damage win.
    let stale_chrome_band = Rect::new(40.0, 40.0, 20.0, 20.0); // inside chrome, away from moved shape
    assert!(
        !intersects(stale_chrome_band),
        "unchanged chrome interior must not enter damage; \
         stale_band = {stale_chrome_band:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Chrome authoring change (hover fill flip, no rect change) must
/// push the chrome rect even though the geometric rect is identical.
/// Chrome is row 0 of the node's paint span and carries its own
/// authoring hash via `Paint.hash`; without that, a hover-color flip
/// would fall through the rect-only guard and emit no damage at all.
#[test]
fn chrome_authoring_change_pushes_chrome_paint_row() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |fill: Color, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("c"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: fill.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut h, |ui| build(BLUE, ui));
    frame(&mut h, |ui| build(BLUE, ui)); // settle
    let snap = h.ui.damage_engine.prev[&WidgetId::from_hash("c")];
    let snap_rect = h.ui.damage_engine.paints.slots[snap.paint_span.start as usize].screen;

    frame(&mut h, |ui| build(RED, ui));
    let region = h.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    assert!(
        rects.iter().any(|r| r.intersects(snap_rect)),
        "chrome authoring change must push chrome paint row even when \
         rect geometry is unchanged; region = {rects:?}",
    );
}

/// Painting-only invariant: every `DamageEngine.prev` entry covers
/// at least one Paint row. A chrome-only owner used to land in `prev`
/// with `shape_span.len == 0` (chrome was tracked in a separate
/// column); under the unified `paint_arena`, chrome is row 0 of the
/// node's span, so the same owner now has `paint_span.len == 1`.
/// The removal tail in `DamageEngine::compute` pushes every prev
/// entry's rows on the frame its widget leaves, so an entry with no rows
/// would leave pixels unrepainted — this test pins the producer side of
/// that contract.
#[test]
fn chrome_only_owner_has_nonzero_paint_span() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("chrome_only"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut h, build);
    frame(&mut h, build); // settle prev

    let wid = WidgetId::from_hash("chrome_only");
    let snap = h.ui.damage_engine.prev[&wid];
    assert_eq!(
        snap.paint_span.len, 1,
        "chrome-only owner must contribute exactly one Paint row (chrome)",
    );

    // Every entry in `prev` covers at least one row.
    for (k, s) in &h.ui.damage_engine.prev {
        assert!(
            s.paint_span.len > 0,
            "prev entry {k:?} has zero-len paint_span, violating painting-only invariant",
        );
    }
}

/// Pin: changing the *content* of a `Shape::Text` with
/// `local_origin: Some(_)` damages the shaped-text bbox, not just the
/// origin point.
///
/// Before the fix, the local bbox for `Text { local_origin: Some(_) }`
/// returned `{ min: origin, size: ZERO }` — a degenerate point, because
/// the glyph extent isn't known to the record. Cascade dutifully stored
/// that point in the shape's paint row; damage then pushed two zero-size
/// rects when text changed → effectively no damage from the text shape. The
/// user-visible symptom: type a character in a `TextEdit`, and only the
/// caret-sized strip got repainted while the rest of the text went
/// stale.
///
/// Post-fix, cascade looks up the shaped extent from
/// `LayerLayout::text_shapes` (already computed by the measure pass)
/// and stores the tight `(origin, measured)` rect. The diff pushes
/// prev + curr extents, so the damage region covers the union of both
/// strings' bboxes.
#[test]
fn text_content_change_damages_shaped_extent_not_just_origin() {
    use crate::scene::node::Node;
    use crate::shape::Shape;
    use crate::text::wrap::TextWrap;
    use crate::text::{FontFamily, FontWeight};

    let mut h = UiHarness::new(DISPLAY.physical);
    // Mono fallback geometry: glyph width = font_size_px * 0.5, line
    // height = font_size_px. With font_size_px = 14, "abc" measures
    // 21×14 and "abcdef" measures 42×14.
    const FONT: f32 = 14.0;
    const ORIGIN: Vec2 = Vec2::new(10.0, 10.0);
    let leaf_id = WidgetId::from_hash("text-host");
    let build = |text: &'static str, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
            .show(ui, |ui| {
                let node = Node::leaf().id(leaf_id);
                ui.widget(node).record(ui, None, |ui| {
                    let text = ui.intern(text);
                    ui.add_shape(
                        Shape::text(
                            text,
                            GlyphFont {
                                line_height_px: FONT,
                                ..GlyphFont::new(FONT)
                            },
                        )
                        .at(ORIGIN)
                        .color(Color::WHITE)
                        .wrap(TextWrap::Truncate)
                        .family(FontFamily::Sans)
                        .weight(FontWeight::Regular),
                    );
                });
            });
    };

    frame(&mut h, |ui| build("abc", ui));
    frame(&mut h, |ui| build("abc", ui));
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "steady frame must produce no diff"
    );

    // Cache prev shaped rect (size of "abc") off the previous snapshot
    // so the assertion below can reason from the actual measured
    // values rather than hand-recomputing mono geometry.
    // Damage rects inflate by `TEXT_SCALE_STEP * measured` total per
    // axis (`STEP/2` per side) to cover composer ladder snaps — see
    // `text_paint_bbox_local`. Expected shaped size scales by the
    // same factor.
    let inflate = 1.0 + TEXT_SCALE_STEP;
    let prev_snap = h.ui.damage_engine.prev[&leaf_id];
    let prev_text_rect = h.ui.damage_engine.paints.slots[prev_snap.paint_span.range()][0].screen;
    let prev_size_short: Size = Size::new(FONT * 0.5 * 3.0 * inflate, FONT * inflate);
    assert!(
        (prev_text_rect.size.w - prev_size_short.w).abs() < 0.5
            && (prev_text_rect.size.h - prev_size_short.h).abs() < 0.5,
        "prev text rect should have shaped size ≈ {prev_size_short:?}, got {prev_text_rect:?}",
    );

    frame(&mut h, |ui| build("abcdef", ui));

    let curr_snap = h.ui.damage_engine.prev[&leaf_id];
    let curr_text_rect = h.ui.damage_engine.paints.slots[curr_snap.paint_span.range()][0].screen;
    let curr_size_long: Size = Size::new(FONT * 0.5 * 6.0 * inflate, FONT * inflate);
    assert!(
        (curr_text_rect.size.w - curr_size_long.w).abs() < 0.5
            && (curr_text_rect.size.h - curr_size_long.h).abs() < 0.5,
        "curr text rect should have shaped size ≈ {curr_size_long:?}, got {curr_text_rect:?}",
    );

    let region = h.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));

    // Probe deep inside the new "abcdef" rect but past where the old
    // "abc" rect ended (x = origin.x + 30 ≈ middle of "abcdef", past
    // the 21-px width of "abc"). Pre-fix this point is *not* in damage
    // (per-shape rect was a zero-size point at origin); post-fix it is
    // (curr rect spans origin..origin+42px).
    let inside_new_only = Rect::new(ORIGIN.x + 30.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_new_only),
        "probe inside new text but past old text must be in damage; \
         probe = {inside_new_only:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Also assert prev's middle gets damaged (so the old glyph
    // pixels actually clear).
    let inside_old = Rect::new(ORIGIN.x + 10.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_old),
        "probe inside old text must be in damage; \
         probe = {inside_old:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin: a visibility flip landing on the SAME frame as a paint-row
/// change must still damage the exact-matched rows. The union push for
/// a `cascade_input` change used to be gated on "every row matched",
/// so hiding a node while one of its shapes was mid-change damaged
/// only the changed shape — the chrome and untouched shapes kept
/// their stale pixels on screen.
#[test]
fn visibility_flip_with_coincident_shape_change_damages_whole_node() {
    // Chrome corner far from the line, so its damage is geometrically
    // distinguishable from the changed shape's.
    const CHROME_PROBE: Rect = Rect::new(44.0, 44.0, 2.0, 2.0);
    const LINE_PROBE: Rect = Rect::new(10.0, 9.0, 2.0, 2.0);
    let node = |ui: &mut Ui, hidden: bool, color: Color| {
        let mut p = Panel::zstack()
            .id(WidgetId::from_hash("a"))
            .size(50.0)
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            });
        if hidden {
            p = p.hidden();
        }
        p.show(ui, |ui| {
            ui.add_shape(
                Shape::line(Vec2::new(5.0, 10.0), Vec2::new(20.0, 10.0), 2.0)
                    .brush(color)
                    .cap(LineCap::Round),
            );
        });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| node(ui, false, BLUE));
    let damage = frame(&mut h, |ui| node(ui, true, RED));
    let region = damage.expect_partial();
    assert!(
        region.any_intersects(LINE_PROBE),
        "changed shape's own rect must be damaged",
    );
    assert!(
        region.any_intersects(CHROME_PROBE),
        "exact-matched chrome must also clear when the node hides; region = {region:?}",
    );
}
