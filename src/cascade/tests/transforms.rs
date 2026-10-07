//! A self-transform's effect on composed rects, stroke fringe and anchoring.

use crate::damage::Damage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;
use glam::Vec2;

/// A direct shape on a panel with `.transform(...)` lands in
/// `Cascade::paint_arenas` at the composed transform (parent ∘ self).
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
    // Shape (0, 0, 30, 30) under parent ∘ self: min = (0,0)*3 + (10,20), size = 30*3 = 90.
    assert_eq!(shape_rect, Rect::new(10.0, 20.0, 90.0, 90.0));
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
                ui.add_shape(Shape::cubic_bezier(
                    Vec2::new(10.0, 20.0),
                    Vec2::new(20.0, 20.0),
                    Vec2::new(30.0, 20.0),
                    Vec2::new(40.0, 20.0),
                    Stroke::new(RgbaF32::WHITE, 4.0),
                ));
            });
        });

        assert_eq!(
            first_paint_screen(&h.ui, "stroke"),
            case.expected,
            "{case:?}"
        );
    }
}

/// `.transform(zoom=S)` on an off-origin panel anchors at the panel's own
/// `layout_rect.min`; unanchored a child would slide by `panel.min * (S - 1)`.
#[test]
fn self_transform_anchors_scale_at_panel_origin() {
    let zoom = 2.0;
    let xform = TranslateScale::from_scale(zoom);

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
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
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 10.0, 10.0))
                            .fill(RgbaF32::srgb(0.5, 0.5, 0.5)),
                    );
                });
        });
    });

    let shape_rect = first_paint_screen(&h.ui, "xpanel");
    // The panel's top-left is the fixed point of its scale: shape at (50, 0),
    // size 10 * 2 = 20. Unanchored it would be (100, 0).
    assert_eq!(
        shape_rect,
        Rect::new(50.0, 0.0, 20.0, 20.0),
        "scale anchors at panel.min, not at the cascade origin",
    );
}

/// The cascade's transform/clip composition (read by hit-test) must agree with
/// the independent recomputation the encoder and composer use to place pixels:
/// a transformed child's composed quad rect equals the cascade's screen rect.
#[test]
fn cascade_screen_rect_matches_composed_quad_under_transform() {
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
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 20.0, 20.0))
                            .fill(RgbaF32::srgb(0.5, 0.5, 0.5)),
                    );
                });
        });
    });

    let cascade_rect = first_paint_screen(&h.ui, "xpanel");

    // Surface scale 1, so physical equals logical; the child's rounded rect is the only quad.
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

    // child-local (0,0,20,20): min = (0,0)*2 + (15,25), size = (20,20)*2 = (40,40)
    assert_eq!(cascade_rect, Rect::new(15.0, 25.0, 40.0, 40.0));
    assert_eq!(
        quad_rect, cascade_rect,
        "the quad lands on the cascade's rect"
    );
}

fn first_paint_screen(ui: &Ui, key: &str) -> Rect {
    let node = ui.cascade().by_id[&WidgetId::from_hash(key)].node;
    let arena = &ui.cascade().layers[Layer::Main].paint_arena;
    let span = arena.node_spans[node.idx()];
    arena.rows[span.start as usize].screen
}
