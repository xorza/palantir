//! What a clip and an overhang do to the region that comes out.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect, size::Size};
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, TEST_SURFACE, frame};
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin: a child whose layout rect overflows a clipped panel (e.g. a
/// scrolled-offscreen row inside a `Scroll` viewport) contributes
/// only its *visible* portion to the damage region. The fix replaces
/// `Cascade.screen_rect` with `Cascade.visible_rect` (raw screen rect
/// intersected with the active ancestor clip) as the damage rect
/// source — without it, panning a long list under a small viewport
/// would inflate the damage union to the full content extent and
/// trip `FULL_REPAINT_THRESHOLD` every frame.
#[test]
fn child_overflowing_clipped_parent_damage_clipped_to_viewport() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let viewport_size = 100.0;
    let child_size = 200.0;
    let build = |fill: RgbaF32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            // Root hstack so the inner zstack honors its `Fixed` size
            // (root nodes get stretched to the surface anchor by the
            // layout engine, which would defeat the clip).
            Panel::hstack()
                .id(WidgetId::from_hash("clip-host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("clip-root"))
                        .size((Sizing::fixed(viewport_size), Sizing::fixed(viewport_size)))
                        .clip_rect()
                        .show(ui, |ui| {
                            *child = Some(
                                Frame::new()
                                    .id(WidgetId::from_hash("overflow"))
                                    .size(child_size)
                                    .background(Background {
                                        fill: fill.into(),
                                        ..Default::default()
                                    })
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        });
    };

    build(BLUE, &mut h, &mut child_node);
    // Authoring change on the child only — fill flips. The child's
    // layout rect is `child_size × child_size` (way past the clip),
    // but the damage rect must stay inside the parent's clip.
    build(RED, &mut h, &mut child_node);

    let region = h.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.size.w <= viewport_size + 0.5 && damage_rect.size.h <= viewport_size + 0.5,
        "damage rect must be clipped to the {viewport_size}px viewport; got {damage_rect:?}",
    );
}

/// Pin: a node that paints a drop shadow contributes its **inflated**
/// paint bounds (`rect + offset`, then `3σ + max(spread, 0)` on each side) to the
/// damage region, not just the arranged rect. Both routes — direct
/// `Shape::Shadow` push and `Background::shadow` chrome — must reach
/// the same `paint_rect` so a tab swap clears the full halo, not just
/// the layout rect.
#[test]
fn drop_shadow_overhang_contributes_to_damage_on_remove() {
    use crate::Shadow;

    let frame_size = 50.0;
    let expected_paint_size = frame_size + 2.0 * (3.0 * 8.0 + 2.0);

    type Build = fn(&mut Ui);
    let cases: &[(&str, Build)] = &[
        ("shape", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                            offset: Vec2::new(12.0, -7.0),
                            blur: 8.0,
                            spread: 2.0,
                            inset: false,
                        })
                        .corners(0.0),
                    );
                });
        }),
        ("chrome", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    shadow: Shadow {
                        color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                        offset: Vec2::new(12.0, -7.0),
                        blur: 8.0,
                        spread: 2.0,
                        inset: false,
                    },
                    ..Default::default()
                })
                .show(ui, |_| {});
        }),
    ];
    for (label, build) in cases {
        let mut h = UiHarness::new(DISPLAY.physical);
        frame(&mut h, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, build);
        });
        let prev_rect = h
            .engines
            .damage
            .prev_paint_rect(WidgetId::from_hash("card"))
            .expect("card painted last frame");
        assert_eq!(
            prev_rect.size,
            Size::new(expected_paint_size, expected_paint_size),
            "[{label}] offset moves the paint bbox without enlarging it",
        );

        frame(&mut h, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        });
        let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
        // `DamageEngine::prev` stores the raw paint_rect including
        // the shadow halo, which extends off the top-left of the
        // 200×200 surface for a 50×50 frame at origin. The damage
        // region, however, clips each rect to the surface in
        // `collapse_from` (off-surface pixels can never be painted
        // and would bias the Full-repaint threshold), so the emitted
        // damage is the visible portion of `prev_rect`.
        assert_eq!(
            rects,
            vec![prev_rect.clamp_to(TEST_SURFACE)],
            "[{label}] damage region",
        );
    }
}

/// Pin: a drop-shadow whose halo extends past a clipping ancestor
/// contributes only the **clipped** halo to damage. The shadow's
/// overhang is folded into `paint_rect` in owner-local space before
/// the ancestor clip is applied, so a `ClipMode::Clip` parent caps
/// the contribution at the parent's bounds — otherwise the halo
/// pretends to paint pixels the GPU's scissor will discard.
#[test]
fn shadow_overhang_inside_clipped_parent_is_clamped() {
    use crate::Shadow;

    let viewport = 60.0;
    let card = 40.0;
    let blur = 8.0;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    let build = |fill: RgbaF32, h: &mut UiHarness| {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("viewport"))
                        .size((Sizing::fixed(viewport), Sizing::fixed(viewport)))
                        .clip_rect()
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("card"))
                                .size((Sizing::fixed(card), Sizing::fixed(card)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui, |ui| {
                                    ui.add_shape(
                                        Shape::shadow(Shadow {
                                            color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                                            offset: Vec2::ZERO,
                                            blur,
                                            spread: 0.0,
                                            inset: false,
                                        })
                                        .corners(0.0),
                                    );
                                });
                        });
                });
        });
    };

    build(BLUE, &mut h);
    build(RED, &mut h);

    for r in h.damage_region().iter_rects() {
        assert!(
            r.size.w <= viewport + 0.5 && r.size.h <= viewport + 0.5,
            "shadow halo damage must stay inside the {viewport}px clip; got {r:?}",
        );
    }
}

/// Pin: a direct shape on a clipped node has its per-shape rect (the
/// column the damage diff reads from) clipped to the node's own clip
/// mask — not just the ancestor clip.
///
/// Before the fix, `compute_paint_rect` clipped each shape's screen
/// rect to `parent_clip` only. A `Shape::Text` with `local_origin`
/// expressing a scroll offset reported its **full** shaped extent as
/// the per-shape rect (cosmic-text's measured `Size` for the whole
/// buffer). For a multi-line `TextEdit` taller than its visible rect,
/// scrolling produced damage rects spanning the entire text — way
/// past the editor's own `ClipMode::Rect`. The encoder's GPU scissor
/// clips the actual pixels, so the user *saw* tight repaints, but
/// the damage region driving the scissor pass was over-large,
/// inflating the partial-redraw quad to the unclipped text bbox.
///
/// This test fakes the scenario with a rounded-rect shape extending
/// past the host's clip on the right edge; pre-fix the per-shape rect
/// captures the full 400-px-wide shape, post-fix it's clipped to the
/// host's deflated mask.
#[test]
fn direct_shape_on_clipped_node_clips_to_own_mask() {
    // WindowDriver panel: 80×40, padding 4 each side via background. The
    // direct shape extends to x=400 (well past 80). After the cascade
    // walk, `shape_rects[idx]` must be clipped to the host's deflated
    // mask, not span the full 400 px.
    let mut h = UiHarness::new(DISPLAY.physical);
    let host_id = WidgetId::from_hash("clip-host");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(host_id)
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .clip_rect()
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 400.0, 20.0))
                            .fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                    );
                });
        });
    };
    frame(&mut h, build);
    frame(&mut h, build);

    // Locate the host node by widget id and read its first shape's
    // cascaded screen rect. Pre-fix the rect spans the full 400 px;
    // post-fix it's clamped to (host_width − padding-fold).
    let cascade = &h.ui.cascade();
    let host_ep = *cascade.by_id.get(&host_id).expect("host node recorded");
    let host_entry_idx = (cascade.layers[host_ep.layer].entries_base + host_ep.node.0) as usize;
    let host_rect = cascade.entries[host_entry_idx].rect;
    let tree = h.ui.tree(Layer::Main);
    let shape_span = tree.records.shape_span()[host_ep.node.idx()];
    assert_eq!(shape_span.len, 1, "the fixture adds one shape to the host");
    // The host paints chrome (the BLUE background), so row 0 of its
    // span is the chrome `Paint` — whose screen always equals the
    // 80×40 arranged rect and would pass the assertion below even
    // with the clip regressed. The direct shape under test is row 1.
    let paint_arena = &cascade.layers[Layer::Main].paint_arena;
    let node_span = paint_arena.node_spans[host_ep.node.idx()];
    assert_eq!(node_span.len, 2, "chrome row + shape row");
    let shape_rect = paint_arena.rows[node_span.start as usize + 1].screen;
    assert!(
        shape_rect.size.w <= host_rect.size.w + 0.5,
        "direct shape rect must be clipped to the host's own mask; \
         host_rect = {host_rect:?}, shape_rect = {shape_rect:?}",
    );
}
