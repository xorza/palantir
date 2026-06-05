use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::ui::frame_report::RenderPlan;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// A direct shape recorded on a panel with `.transform(...)` must
/// land in `Cascades::paint_arenas` at the *composed* transform
/// (parent ∘ self), not just `parent_transform`. Pins the cascade
/// half of the `Panel::transform`-applies-to-body contract — the
/// encoder half is already pinned by
/// `transformed_panel_applies_transform_to_direct_shapes`.
#[test]
fn shape_rect_composes_self_transform() {
    let scale = 3.0;
    let translate = Vec2::new(10.0, 20.0);
    let xform = TranslateScale::new(translate, scale);

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::Fixed(300.0))
                .transform(xform)
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(0.0, 0.0, 30.0, 30.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let xpanel = cascades.by_id[&WidgetId::from_hash("xpanel")].node;
    let span = cascades.layers[layer].paint_arena.node_spans[xpanel.idx()];
    let shape_rect = cascades.layers[layer].paint_arena.rows[span.start as usize].screen;
    // The Panel sits at the hstack origin (0, 0). Owner-local
    // shape rect is (0, 0, 30, 30); after `parent ∘ self`:
    //   min = (0, 0) * 3 + (10, 20) = (10, 20)
    //   size = (30, 30) * 3 = (90, 90)
    let eps = 1e-3;
    assert!(
        (shape_rect.min.x - 10.0).abs() < eps
            && (shape_rect.min.y - 20.0).abs() < eps
            && (shape_rect.size.w - 90.0).abs() < eps
            && (shape_rect.size.h - 90.0).abs() < eps,
        "expected shape_rect = (10, 20, 90, 90); got {shape_rect:?}",
    );
}

/// `.transform(zoom=S)` on an off-origin panel must anchor the
/// scale at the panel's own `layout_rect.min`, not at the
/// cascade's (0, 0). A child at panel-local (0, 0) should land
/// at the panel's origin regardless of `S` — without anchoring it
/// would slide off by `panel.min * (S - 1)`. Pins the cascade-
/// level half of the "scale my body about my own origin"
/// `Panel::transform` contract.
#[test]
fn self_transform_anchors_scale_at_panel_origin() {
    let zoom = 2.0;
    let xform = TranslateScale::from_scale(zoom);

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        // Push the transformed panel off the surface origin with a
        // leading sibling — Spacer-style placeholder so the panel
        // sits at (sibling_width, 0) instead of (0, 0).
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("spacer"))
                .size(Sizing::Fixed(50.0))
                .show(ui, |_| {});
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::Fixed(200.0))
                .transform(xform)
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        // Panel-local (0, 0) — the natural top-left
                        // of the panel's body.
                        local_rect: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let xpanel = cascades.by_id[&WidgetId::from_hash("xpanel")].node;
    let span = cascades.layers[layer].paint_arena.node_spans[xpanel.idx()];
    let shape_rect = cascades.layers[layer].paint_arena.rows[span.start as usize].screen;
    // Panel sits at (50, 0). Shape's panel-local (0, 0) should
    // map to screen (50, 0) under the anchor — the panel's own
    // top-left is the fixed point of its scale. Size is
    // `panel-local size * zoom = 10 * 2 = 20`.
    //
    // Without anchoring, the raw `parent.compose(self).apply(panel.min)`
    // would give `(50, 0) * 2 = (100, 0)` — content slides 50px
    // right of where it belongs.
    let eps = 1e-3;
    assert!(
        (shape_rect.min.x - 50.0).abs() < eps && (shape_rect.min.y - 0.0).abs() < eps,
        "expected shape min = (50, 0); got {:?} — scale should anchor at panel.min, \
         not at cascade origin",
        shape_rect.min,
    );
    assert!(
        (shape_rect.size.w - 20.0).abs() < eps && (shape_rect.size.h - 20.0).abs() < eps,
        "expected size = (20, 20) (panel-local * zoom); got {:?}",
        shape_rect.size,
    );
}

/// A panel with chrome emits a Paint row at the start of its
/// node's `node_spans` span; a chromeless panel emits an empty
/// span.
#[test]
fn node_spans_populated_for_chrome_panels_only() {
    use crate::primitives::background::Background;

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("chrome"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                    ..Default::default()
                })
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("bare"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui, |_| {});
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let chrome_idx = cascades.by_id[&WidgetId::from_hash("chrome")].node.idx();
    let bare_idx = cascades.by_id[&WidgetId::from_hash("bare")].node.idx();
    let chrome_span = cascades.layers[layer].paint_arena.node_spans[chrome_idx];
    let bare_span = cascades.layers[layer].paint_arena.node_spans[bare_idx];

    assert!(
        chrome_span.len > 0
            && cascades.layers[layer].paint_arena.rows[chrome_span.start as usize]
                .screen
                .area()
                > 0.0,
        "chromed panel must have a non-empty paint span with non-zero chrome rect",
    );
    assert_eq!(
        bare_span.len, 0,
        "chromeless panel must have empty paint span; got {bare_span:?}",
    );
}

/// `node_spans` length matches the layer's node count so the
/// damage diff can index by `NodeId.0` without a bounds-cap.
#[test]
fn node_spans_sized_to_node_count() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack().auto_id().show(ui, |_| {});
        });
    });
    let layer = Layer::Main;
    let nodes = ui.forest.tree(Layer::Main).records.len();
    assert_eq!(
        ui.layout.cascades.layers[layer]
            .paint_arena
            .node_spans
            .len(),
        nodes,
        "node_spans column must be sized to the layer's node count",
    );
}

/// Cross-check that the cascade's transform/clip composition (which
/// hit-test consumes via `paint_arena` / `EntryRow.rect`) agrees with
/// the *independent* recomputation the encoder + composer perform to
/// place the actual pixels. They are separate code paths — the encoder
/// recomputes transform/clip from the tree rather than reading cascade
/// output (`encoder/mod.rs`), kept in lockstep only by sharing the
/// `TranslateScale`/`Rect` primitives. This pins that they don't drift:
/// a transformed child's *composed quad rect* must equal the cascade's
/// *screen rect* for that shape. A `ClipMode::Rect` is in the pipeline
/// (exercises the encoder's clip-push + the composer's scissor) but the
/// child sits fully inside the panel, so the clip doesn't reduce the
/// painted geometry and the comparison stays apples-to-apples.
#[test]
fn cascade_screen_rect_matches_composed_quad_under_transform() {
    // translate=(15,25), scale=2 — non-trivial on both axes.
    let xform = TranslateScale::new(Vec2::new(15.0, 25.0), 2.0);

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(400, 400), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::Fixed(300.0))
                .clip(ClipMode::Rect)
                .transform(xform)
                .show(ui, |ui| {
                    // Fully inside the 300×300 panel → clip never bites.
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(0.0, 0.0, 20.0, 20.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    });

    // Cascade's screen rect for the child shape (what hit-test sees).
    let layer = Layer::Main;
    let cascade_rect = {
        let cascades = &ui.layout.cascades;
        let xpanel = cascades.by_id[&WidgetId::from_hash("xpanel")].node;
        let span = cascades.layers[layer].paint_arena.node_spans[xpanel.idx()];
        cascades.layers[layer].paint_arena.rows[span.start as usize].screen
    };

    // Composer's actual painted quad. Surface scale = 1, so physical px
    // == logical px and the rect compares directly. The transparent
    // viewport / hstack / canvas chrome emit no quads — the child
    // RoundedRect is the only one.
    let mut frontend = crate::renderer::frontend::Frontend::for_test();
    let buffer = frontend.build(
        &ui,
        RenderPlan::Full {
            clear: ui.theme.window_clear,
        },
    );
    assert_eq!(
        buffer.quads.len(),
        1,
        "expected exactly the child quad; got {:?}",
        buffer.quads,
    );
    let quad_rect = buffer.quads[0].rect;

    // child-local (0,0,20,20) under (translate=(15,25), scale=2):
    //   min = (0,0)*2 + (15,25) = (15,25);  size = (20,20)*2 = (40,40)
    let eps = 1e-3;
    assert!(
        (cascade_rect.min.x - 15.0).abs() < eps
            && (cascade_rect.min.y - 25.0).abs() < eps
            && (cascade_rect.size.w - 40.0).abs() < eps
            && (cascade_rect.size.h - 40.0).abs() < eps,
        "cascade screen rect wrong: {cascade_rect:?} (expected min (15,25) size (40,40))",
    );
    assert!(
        (quad_rect.min.x - cascade_rect.min.x).abs() < eps
            && (quad_rect.min.y - cascade_rect.min.y).abs() < eps
            && (quad_rect.size.w - cascade_rect.size.w).abs() < eps
            && (quad_rect.size.h - cascade_rect.size.h).abs() < eps,
        "composer quad {quad_rect:?} drifted from cascade screen rect {cascade_rect:?} — \
         encoder/composer transform composition diverged from the cascade walk",
    );
}

/// Stage 1 incremental cascade: a localized change (one leaf's size)
/// must skip every unchanged sibling subtree and recompute only the
/// spine down to the change. The cross-check (`assert_cascades_eq`, run
/// automatically on every incremental frame under test) already proves
/// the *output* equals a full recompute; this pins that the skip gate
/// actually *fires* so the work is genuinely saved.
#[test]
fn incremental_skips_unchanged_sibling_subtrees() {
    const SIBLINGS: usize = 6;
    fn build(ui: &mut Ui, footer_w: f32) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                for i in 0..SIBLINGS {
                    Panel::vstack()
                        .id(WidgetId::from_hash(("sib", i)))
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash(("leaf", i)))
                                .size((Sizing::Fixed(10.0), Sizing::Fixed(10.0)))
                                .show(ui, |_| {});
                        });
                }
                Panel::hstack()
                    .id(WidgetId::from_hash("footer"))
                    .size((Sizing::Fixed(footer_w), Sizing::Fixed(10.0)))
                    .show(ui, |_| {});
            });
    }

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 30.0));
    assert!(
        !ui.cascades_engine.dbg.incremental,
        "first cascade run has no prev — must be a full recompute",
    );

    // Frame 2: only the footer's width changes. Its subtree_hash (and
    // every ancestor's) flips, but the six sibling subtrees' authoring
    // and arranged origins are untouched, so each is bulk-copied as a
    // unit while only the spine (root → footer) is recomputed.
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 80.0));
    let dbg = ui.cascades_engine.dbg;
    assert!(
        dbg.incremental,
        "stable structure + a change ⇒ incremental path",
    );
    // Each of the 6 static sibling subtrees is bulk-copied as one unit;
    // only the spine that actually changed (the surface/root ancestors
    // + the footer leaf) is recomputed.
    assert_eq!(
        dbg.skipped, SIBLINGS as u32,
        "one skip per static sibling subtree; got {dbg:?}",
    );
    let total = ui.forest.tree(Layer::Main).records.len() as u32;
    assert!(
        dbg.recascaded > 0 && dbg.recascaded < total,
        "incremental must recompute the changed spine only (1..{total}); got {dbg:?}",
    );
}

/// A structural change (different node count / id mapping) invalidates
/// NodeId-indexed reuse, so the frame must fall back to a full
/// recompute rather than risk copying a prev row into the wrong node.
#[test]
fn incremental_falls_back_to_full_on_structure_change() {
    fn build(ui: &mut Ui, n: usize) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                for i in 0..n {
                    Panel::hstack()
                        .id(WidgetId::from_hash(("child", i)))
                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                        .show(ui, |_| {});
                }
            });
    }
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 3));
    // Frame 2 adds a 4th child: node count + id mapping shift.
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 4));
    assert!(
        !ui.cascades_engine.dbg.incremental,
        "structure change must drop to the full recompute path",
    );
}

/// A node whose authoring is unchanged can still *move* when an earlier
/// sibling reflows — `subtree_hash` (authoring only) won't catch it, so
/// the skip gate also compares the arranged rect. Without that compare
/// `body` would be wrongly skipped and keep last frame's stale position.
/// (The automatic cross-check would also catch this; the explicit
/// assertion names the exact subtlety.)
#[test]
fn incremental_busts_skip_when_unchanged_sibling_reflows() {
    fn body_y(ui: &Ui) -> f32 {
        let idx = ui
            .layout
            .cascades
            .entry_idx_of(WidgetId::from_hash("body"))
            .expect("body recorded") as usize;
        ui.layout.cascades.entries.layout_rect()[idx].min.y
    }
    fn build(ui: &mut Ui, head_h: f32) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("head"))
                    .size((Sizing::Fixed(30.0), Sizing::Fixed(head_h)))
                    .show(ui, |_| {});
                // Identical authoring every frame, but pushed down when
                // `head` grows — a moved-but-unchanged subtree.
                Panel::hstack()
                    .id(WidgetId::from_hash("body"))
                    .size((Sizing::Fixed(30.0), Sizing::Fixed(30.0)))
                    .show(ui, |_| {});
            });
    }
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 40.0));
    let y1 = body_y(&ui);
    ui.run_at_acked(UVec2::new(200, 400), |ui| build(ui, 90.0));
    assert!(
        ui.cascades_engine.dbg.incremental,
        "stable structure ⇒ incremental path",
    );
    let y2 = body_y(&ui);
    assert_eq!(
        y2 - y1,
        50.0,
        "body must follow head's +50 growth (origin gate must bust the stale-skip); \
         got y1={y1} y2={y2}",
    );
}
