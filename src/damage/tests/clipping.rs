//! What a clip and an overhang do to the region that comes out.

use crate::Ui;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
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

/// A child whose layout rect overflows a clipped panel (e.g. a scrolled-away row
/// in a `Scroll`) contributes only its *visible* portion to damage: the damage
/// rect source is `Cascade.visible_rect` (screen rect clipped by the ancestor
/// clip), else panning a long list would trip `FULL_REPAINT_THRESHOLD` every
/// frame.
#[test]
fn child_overflowing_clipped_parent_damage_clipped_to_viewport() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut child_node = None;
    let viewport_size = 100.0;
    let child_size = 200.0;
    let build = |fill: RgbaF32, h: &mut UiHarness, child: &mut Option<NodeId>| {
        h.frame(|ui| {
            // Root hstack so the inner zstack honors its `Fixed` size; root nodes are
            // stretched to the surface anchor, which would defeat the clip.
            Panel::hstack()
                .id(WidgetId::from_hash("clip-host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("clip-root"))
                        .size((Sizing::fixed(viewport_size), Sizing::fixed(viewport_size)))
                        .clip_rect()
                        .show(ui, |ui| {
                            *child = Some(
                                Block::new()
                                    .id(WidgetId::from_hash("overflow"))
                                    .size(child_size)
                                    .background(Background::fill(fill))
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        });
    };

    build(BLUE, &mut h, &mut child_node);
    // Authoring change on the child only (fill flips). Its layout rect is far past
    // the clip, but the damage rect must stay inside the parent's clip.
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

/// A node painting a drop shadow contributes its **inflated** paint bounds
/// (`rect + offset`, then `4*sigma + max(spread, 0)` per side) to damage. Both
/// `Shape::Shadow` and `Background::shadow` chrome must reach the same
/// `paint_rect`, so a tab swap clears the full halo.
#[test]
fn drop_shadow_overhang_contributes_to_damage_on_remove() {
    type Build = fn(&mut Ui);

    use crate::Shadow;

    let frame_size = 50.0;
    let expected_paint_size = frame_size + 2.0 * (4.0 * 8.0 + 2.0);

    let cases: &[(&str, Build)] = &[
        ("shape", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .background(Background::fill(BLUE))
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
        // `DamageEngine::prev` stores the raw paint_rect including the halo, which
        // extends off the top-left of the 200x200 surface. The damage region clips each
        // rect to the surface in `collapse_from` (off-surface pixels can't be painted
        // and would bias the Full-repaint threshold), so the damage is the visible part.
        assert_eq!(
            rects,
            vec![prev_rect.clamp_to(DISPLAY.logical_rect())],
            "[{label}] damage region",
        );
    }
}

/// A drop-shadow halo past a clipping ancestor contributes only the **clipped**
/// halo to damage: the overhang is folded into `paint_rect` in owner-local space
/// before the ancestor clip, so a `ClipMode::Clip` parent caps it at its bounds.
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
                                .background(Background::fill(fill))
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

/// A direct shape on a clipped node has its per-shape rect (the column the
/// damage diff reads) clipped to the node's own clip mask, not just the ancestor
/// clip. `compute_paint_rect` once clipped to `parent_clip` only, so a
/// `Shape::Text` with a scroll `local_origin` reported its full shaped extent and
/// a scrolling multi-line `TextEdit` produced damage spanning the entire text.
///
/// Faked with a rounded-rect shape extending past the host's clip on the right;
/// the per-shape rect must be the host's deflated mask, not the full 400 px.
#[test]
fn direct_shape_on_clipped_node_clips_to_own_mask() {
    // WindowDriver panel: 80x40, padding 4 per side via background. The direct
    // shape extends to x=400; after the cascade walk `shape_rects[idx]` must be
    // clipped to the host's deflated mask.
    let mut h = UiHarness::new(DISPLAY.physical);
    let host_id = WidgetId::from_hash("clip-host");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(host_id)
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .background(Background::fill(BLUE))
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

    // Read the host node's first shape's cascaded screen rect: it must be clamped
    // to (host_width - padding-fold), not span 400 px.
    let cascade = &h.ui.cascade();
    let host_ep = *cascade.by_id.get(&host_id).expect("host node recorded");
    let host_entry_idx = (cascade.layers[host_ep.layer].entries_base + host_ep.node.0) as usize;
    let host_rect = cascade.entries[host_entry_idx].rect;
    let tree = h.ui.tree(Layer::Main);
    let shape_span = tree.records.shape_span()[host_ep.node.idx()];
    assert_eq!(shape_span.len, 1, "the fixture adds one shape to the host");
    // The host paints chrome (the BLUE background), so row 0 is the chrome `Paint`,
    // whose screen is always the 80x40 arranged rect and would pass even with the
    // clip regressed. The shape under test is row 1.
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

/// A transparent container with a rounded clip keeps a chrome row only so the
/// mask can read its corners. The row paints nothing, so adding the container
/// damages only its child: a 100x100 container holding a 20x20 child at its
/// top-left, added in one frame, damages exactly 20x20.
#[test]
fn a_transparent_rounded_clip_damages_nothing_of_its_own() {
    use crate::primitives::geometry::corners::Corners;

    const CHILD: f32 = 20.0;
    let build = |ui: &mut Ui, with_host: bool| {
        Panel::hstack().auto_id().show(ui, |ui| {
            if !with_host {
                return;
            }
            Panel::zstack()
                .id(WidgetId::from_hash("rounded-host"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .background(Background {
                    corners: Corners::all(12.0),
                    ..Default::default()
                })
                .clip_rounded()
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("inner"))
                        .size(CHILD)
                        .background(Background::fill(BLUE))
                        .show(ui);
                });
        });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| build(ui, false));
    frame(&mut h, |ui| build(ui, true));

    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(0.0, 0.0, CHILD, CHILD)]);
}
