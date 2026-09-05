//! A self-transform's effect on composed rects, stroke fringe, and
//! anchoring.

use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;

use crate::Ui;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;
use glam::Vec2;

/// A direct shape recorded on a panel with `.transform(...)` must
/// land in `Cascade::paint_arenas` at the *composed* transform
/// (parent ∘ self), not just `parent_transform`. Pins the cascade
/// half of the `Panel::transform`-applies-to-body contract — the
/// encoder half is already pinned by
/// `transformed_panel_applies_transform_to_direct_shapes`.
#[test]
fn shape_rect_composes_self_transform() {
    let scale = 3.0;
    let translate = Vec2::new(10.0, 20.0);
    let xform = TranslateScale::new(translate, scale);

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(300.0))
                .transform(xform)
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 30.0, 30.0))
                            .fill(RgbaF32::srgb(0.5, 0.5, 0.5)),
                    );
                });
        });
    });

    let shape_rect = first_paint_screen(&h.ui, "xpanel");
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
        let mut h = UiHarness::new(UVec2::splat(400)).scale(case.display_scale);
        h.frame(|ui| {
            let mut panel = Panel::canvas()
                .id(WidgetId::from_hash("stroke"))
                .size(Sizing::fixed(case.panel_size))
                .transform(TranslateScale::from_scale(case.transform_scale));
            if case.clipped {
                panel = panel.clip(ClipMode::Rect);
            }
            panel.show(ui, |ui| {
                ui.add_shape(
                    Shape::cubic_bezier(
                        Vec2::new(10.0, 20.0),
                        Vec2::new(20.0, 20.0),
                        Vec2::new(30.0, 20.0),
                        Vec2::new(40.0, 20.0),
                        4.0,
                    )
                    .brush(RgbaF32::WHITE),
                );
            });
        });

        assert_eq!(
            first_paint_screen(&h.ui, "stroke"),
            case.expected,
            "{case:?}"
        );
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

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
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
                    // Panel-local (0, 0) — the natural top-left
                    // of the panel's body.
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 10.0, 10.0))
                            .fill(RgbaF32::srgb(0.5, 0.5, 0.5)),
                    );
                });
        });
    });

    let shape_rect = first_paint_screen(&h.ui, "xpanel");
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

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(300.0))
                .clip(ClipMode::Rect)
                .transform(xform)
                .show(ui, |ui| {
                    // Fully inside the 300×300 panel → clip never bites.
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 20.0, 20.0))
                            .fill(RgbaF32::srgb(0.5, 0.5, 0.5)),
                    );
                });
        });
    });

    // Cascade's screen rect for the child shape (what hit-test sees).
    let cascade_rect = first_paint_screen(&h.ui, "xpanel");

    // Composer's actual painted quad. Surface scale = 1, so physical px
    // == logical px and the rect compares directly. The transparent
    // viewport / hstack / canvas chrome emit no quads — the child
    // A rounded rect is the only one.
    let mut frontend = Frontend::for_test();
    frontend.build(
        h.ui.frame_scene(),
        RenderPlan {
            clear: h.ui.theme().window_clear,
            damage: Damage::Full,
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

/// Screen rect of the first paint row for the widget keyed by
/// `WidgetId::from_hash(key)` on `Layer::Main`.
fn first_paint_screen(ui: &Ui, key: &str) -> Rect {
    let node = ui.cascade().by_id[&WidgetId::from_hash(key)].node;
    let arena = &ui.cascade().layers[Layer::Main].paint_arena;
    let span = arena.node_spans[node.idx()];
    arena.rows[span.start as usize].screen
}
