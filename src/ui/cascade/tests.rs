use crate::display::Display;
use crate::forest::element::Configure;
use crate::forest::layer::Layer;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{LineCap, Shape};
use crate::ui::cascade::{CascadePrefixBits, build_cascade_prefix, finish_cascade_input};
use crate::ui::frame::FrameStamp;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::widgets::panel::Panel;
use crate::{Ui, renderer::frontend::Frontend};
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Screen rect of the first paint row for the widget keyed by
/// `WidgetId::from_hash(key)` on `Layer::Main`.
fn first_paint_screen(ui: &Ui, key: &str) -> Rect {
    let node = ui.cascades.by_id[&WidgetId::from_hash(key)].node;
    let arena = &ui.cascades.layers[Layer::Main].paint_arena;
    let span = arena.node_spans[node.idx()];
    arena.rows[span.start as usize].screen
}

#[test]
fn cascade_input_hash_collapses_visual_zero_noise() {
    use crate::primitives::approx::EPS;

    assert_eq!(std::mem::size_of::<CascadePrefixBits>(), 32);
    let hash = |transform, rect| {
        let prefix = build_cascade_prefix(transform, None, false, false);
        finish_cascade_input(&prefix, rect, false)
    };
    let baseline = hash(TranslateScale::IDENTITY, Rect::ZERO);
    assert_eq!(
        baseline,
        hash(
            TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5),
            Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS),
        ),
    );
    assert_ne!(
        baseline,
        hash(
            TranslateScale::from_translation(Vec2::new(EPS * 2.0, 0.0)),
            Rect::ZERO,
        ),
    );
}

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
                .size(Sizing::fixed(300.0))
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

    let shape_rect = first_paint_screen(&ui, "xpanel");
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

#[test]
fn stroke_bbox_inflates_after_transform_with_physical_fringe() {
    #[derive(Debug)]
    struct Case {
        transform_scale: f32,
        display_scale: f32,
        panel_size: f32,
        clipped: bool,
        expected: Rect,
    }

    let cases = [
        // centerline=(5,10)..(20,10), half-width=1, fringe=0.5
        Case {
            transform_scale: 0.5,
            display_scale: 1.0,
            panel_size: 300.0,
            clipped: false,
            expected: Rect::new(3.5, 8.5, 18.0, 3.0),
        },
        // centerline=(10,20)..(40,20), half-width=2, fringe=0.25
        Case {
            transform_scale: 1.0,
            display_scale: 2.0,
            panel_size: 300.0,
            clipped: false,
            expected: Rect::new(7.75, 17.75, 34.5, 4.5),
        },
        // centerline=(20,40)..(80,40), half-width=4, fringe=1
        Case {
            transform_scale: 2.0,
            display_scale: 0.5,
            panel_size: 300.0,
            clipped: false,
            expected: Rect::new(15.0, 35.0, 70.0, 10.0),
        },
        // unclipped stroke=(7.5,17.5)..(42.5,22.5), clamped to x≤30
        Case {
            transform_scale: 1.0,
            display_scale: 1.0,
            panel_size: 30.0,
            clipped: true,
            expected: Rect::new(7.5, 17.5, 22.5, 5.0),
        },
    ];

    for case in cases {
        let mut ui = Ui::for_test();
        let display = Display::from_physical(UVec2::splat(400), case.display_scale);
        ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            let mut panel = Panel::canvas()
                .id(WidgetId::from_hash("stroke"))
                .size(Sizing::fixed(case.panel_size))
                .transform(TranslateScale::from_scale(case.transform_scale));
            if case.clipped {
                panel = panel.clip(ClipMode::Rect);
            }
            panel.show(ui, |ui| {
                ui.add_shape(Shape::CubicBezier {
                    p0: Vec2::new(10.0, 20.0),
                    p1: Vec2::new(20.0, 20.0),
                    p2: Vec2::new(30.0, 20.0),
                    p3: Vec2::new(40.0, 20.0),
                    width: 4.0,
                    brush: Color::WHITE.into(),
                    cap: LineCap::Butt,
                });
            });
        });

        assert_eq!(first_paint_screen(&ui, "stroke"), case.expected, "{case:?}");
    }
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
                .size(Sizing::fixed(50.0))
                .show(ui, |_| {});
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(200.0))
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

    let shape_rect = first_paint_screen(&ui, "xpanel");
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

/// A panel with chrome emits a Paint row at the start of its node's
/// `node_spans` span; a chromeless childless panel emits an empty
/// span; a chromeless *parent* emits one marker row per child — zero
/// screen (markers produce no pixels), hash = the child's `WidgetId`
/// bits (its paint-order identity for the damage diff's row matcher).
#[test]
fn node_spans_rows_mirror_chrome_and_children() {
    use crate::primitives::background::Background;

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("chrome"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .background(Background {
                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                    ..Default::default()
                })
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("bare"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("parent"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("kid"))
                        .size((Sizing::fixed(10.0), Sizing::fixed(10.0)))
                        .show(ui, |_| {});
                });
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.cascades;
    let arena = &cascades.layers[layer].paint_arena;
    let chrome_idx = cascades.by_id[&WidgetId::from_hash("chrome")].node.idx();
    let bare_idx = cascades.by_id[&WidgetId::from_hash("bare")].node.idx();
    let parent_idx = cascades.by_id[&WidgetId::from_hash("parent")].node.idx();
    let chrome_span = arena.node_spans[chrome_idx];
    let bare_span = arena.node_spans[bare_idx];
    let parent_span = arena.node_spans[parent_idx];

    assert!(
        chrome_span.len > 0 && arena.rows[chrome_span.start as usize].screen.area() > 0.0,
        "chromed panel must have a non-empty paint span with non-zero chrome rect",
    );
    let chrome_entry = cascades
        .entry_idx_of(WidgetId::from_hash("chrome"))
        .unwrap() as usize;
    assert_eq!(
        arena.rows[chrome_span.start as usize].screen,
        cascades.entries.rect()[chrome_entry],
        "no-shadow chrome must reuse the node's transformed and clipped visible rect",
    );
    assert_eq!(
        bare_span.len, 0,
        "chromeless childless panel must have empty paint span; got {bare_span:?}",
    );
    assert_eq!(
        parent_span.len, 1,
        "chromeless one-child parent must have exactly its marker row; got {parent_span:?}",
    );
    let marker = arena.rows[parent_span.start as usize];
    assert!(
        marker.screen.is_paint_empty(),
        "child marker row must carry no pixels; got {:?}",
        marker.screen,
    );
    assert_eq!(
        marker.hash.0,
        WidgetId::from_hash("kid").0,
        "child marker hash must be the child's WidgetId bits",
    );
}

/// Every per-node output column follows tree size changes exactly and every
/// retained slot is overwritten with a valid row for the current tree.
#[test]
fn per_node_columns_track_tree_size() {
    let mut ui = Ui::for_test();
    for child_count in [3usize, 1, 4] {
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("column-root"))
                .show(ui, |ui| {
                    for i in 0..child_count {
                        Panel::hstack()
                            .id(WidgetId::from_hash(("column-child", i)))
                            .show(ui, |_| {});
                    }
                });
        });
        let layer = Layer::Main;
        let nodes = ui.forest.trees[layer].records.len();
        let cascades = &ui.cascades.layers[layer];
        assert_eq!(cascades.cascade_inputs.len(), nodes);
        assert_eq!(cascades.subtree_paint_rects.len(), nodes);
        assert_eq!(cascades.subtree_ends.len(), nodes);
        assert_eq!(cascades.paint_arena.node_spans.len(), nodes);
        for (i, (&end, &span)) in cascades
            .subtree_ends
            .iter()
            .zip(&cascades.paint_arena.node_spans)
            .enumerate()
        {
            assert!(end as usize > i && end as usize <= nodes);
            assert!(span.start as usize + span.len as usize <= cascades.paint_arena.rows.len());
        }
    }
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
                .size(Sizing::fixed(300.0))
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
    let cascade_rect = first_paint_screen(&ui, "xpanel");

    // Composer's actual painted quad. Surface scale = 1, so physical px
    // == logical px and the rect compares directly. The transparent
    // viewport / hstack / canvas chrome emit no quads — the child
    // RoundedRect is the only one.
    let mut frontend = Frontend::for_test();
    frontend.build_for_test(
        &ui,
        RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        },
    );
    let buffer = &frontend.buffer;
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

/// A non-painting sibling seeds `Rect::ZERO`; folding it into the
/// parent rollup must not anchor `subtree_paint_rects` at the origin —
/// that would make every ancestor of any layout-only node span
/// `(0,0)..content`, defeating the encoder's subtree cull for content
/// offscreen toward +x/+y.
#[test]
fn non_painting_sibling_does_not_origin_anchor_subtree_rollup() {
    use crate::primitives::background::Background;
    use crate::widgets::frame::Frame;
    use crate::widgets::panel::Panel;
    let row = WidgetId::from_hash("row");
    let mut ui = Ui::for_test();
    ui.run_at(glam::UVec2::new(200, 200), |ui| {
        Panel::hstack().id(row).show(ui, |ui| {
            // Layout-only spacer: occupies 50 px, paints nothing.
            Panel::hstack()
                .id(WidgetId::from_hash("spacer"))
                .size(50.0)
                .show(ui, |_| {});
            Frame::new()
                .id(WidgetId::from_hash("painted"))
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
        });
    });
    let ep = ui.cascades.by_id[&row];
    let rollup = ui.cascades.layers[ep.layer].subtree_paint_rects[ep.node.idx()];
    assert_eq!(
        rollup,
        Rect::new(50.0, 0.0, 50.0, 50.0),
        "spacer's ZERO seed must not drag the rollup's min to the origin",
    );
}

#[test]
fn hit_entries_track_only_sensing_or_focusable_rows_in_paint_order() {
    use crate::input::sense::Sense;
    use crate::widgets::frame::Frame;

    let inert = WidgetId::from_hash("inert");
    let hover = WidgetId::from_hash("hover");
    let focus = WidgetId::from_hash("focus");
    let disabled = WidgetId::from_hash("disabled");
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::splat(100), |ui| {
        Panel::zstack()
            .auto_id()
            .size(Sizing::fixed(100.0))
            .show(ui, |ui| {
                Frame::new().id(inert).size(Sizing::FILL).show(ui);
                Frame::new()
                    .id(hover)
                    .size(Sizing::FILL)
                    .sense(Sense::HOVER)
                    .show(ui);
                Frame::new()
                    .id(focus)
                    .size(Sizing::FILL)
                    .focusable(true)
                    .show(ui);
                Frame::new()
                    .id(disabled)
                    .size(Sizing::FILL)
                    .sense(Sense::CLICK)
                    .focusable(true)
                    .disabled(true)
                    .show(ui);
            });
    });

    assert_eq!(
        ui.cascades.hit_entries,
        [
            ui.cascades.entry_idx_of(hover).unwrap(),
            ui.cascades.entry_idx_of(focus).unwrap(),
        ],
    );
    let pos = Vec2::splat(50.0);
    assert_eq!(ui.cascades.hit_test(pos, Sense::hovers), Some(hover),);
    assert_eq!(ui.cascades.hit_test(pos, Sense::clicks), None);
    assert_eq!(ui.cascades.hit_test_focusable(pos), Some(focus));
    let targets = ui
        .cascades
        .hit_test_targets(pos, Sense::hovers, Sense::scrolls, Sense::pinches);
    assert_eq!(targets.hover, Some(hover));
    assert_eq!(targets.scroll, None);
    assert_eq!(targets.pinch, None);

    ui.run_at_acked(UVec2::splat(100), |ui| {
        Frame::new().id(inert).size(Sizing::FILL).show(ui);
    });
    assert!(ui.cascades.hit_entries.is_empty());
}
