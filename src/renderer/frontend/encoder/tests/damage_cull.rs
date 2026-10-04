//! Which draws survive a partial frame's damage filter.

use crate::Ui;
use crate::damage::region::DamageRegion;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::encoder::tests::support::count_draw_rects;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn damage_filter_partitions_drawrects_by_dirty_region() {
    let cases: &[(&str, Rect, usize)] = &[
        (
            "outside_filter_skipped",
            Rect::new(0.0, 0.0, 30.0, 200.0),
            1,
        ),
        ("inside_filter_kept", Rect::new(0.0, 0.0, 200.0, 200.0), 2),
    ];
    for (label, filter, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                    .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                    .background(Background::fill(RgbaF32::srgb(0.0, 1.0, 0.0)))
                    .show(ui);
            });
        });
        let cmds = h.encode_paint_for(DamageRegion::from(*filter));
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}

/// Cull subtree when filter misses it: clipped or transformed parent's
/// Push/Pop and descendant draws all suppressed. By-convention trust:
/// children stay inside the parent's screen_rect.
#[test]
fn damage_filter_culls_subtree_outside_damage() {
    #[derive(Debug)]
    enum Wrap {
        Clipped,
        Transformed,
    }
    // The whole stream a region over the subtree keeps: the wrap's
    // push, the leaf's quad, the wrap's pop.
    let cases = [
        ("clipped", Wrap::Clipped, ["PushClip", "Quad", "PopClip"]),
        (
            "transformed",
            Wrap::Transformed,
            ["PushTransform", "Quad", "PopTransform"],
        ),
    ];
    for (label, wrap, kept) in cases {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let inner = |ui: &mut Ui| {
                    Block::new()
                        .id(WidgetId::from_hash("inner"))
                        .size(20.0)
                        .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                };
                match wrap {
                    Wrap::Clipped => Panel::hstack()
                        .id(WidgetId::from_hash("clipped"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .clip_rect()
                        .show(ui, inner),
                    Wrap::Transformed => Panel::hstack()
                        .id(WidgetId::from_hash("transformed"))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .transform(TranslateScale::from_translation(Vec2::new(5.0, 5.0)))
                        .show(ui, inner),
                };
            });
        });
        // A region past the subtree culls the push, the pop and the draw;
        // one over it keeps all three, so an encoder that drew nothing at
        // all fails the second row.
        let past = h.encode_paint_for(DamageRegion::from(Rect::new(150.0, 150.0, 50.0, 50.0)));
        assert!(
            past.calls.is_empty(),
            "case {label}: region past the subtree painted {:?}",
            past.kinds()
        );
        let over = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 50.0, 50.0)));
        assert_eq!(over.kinds(), kept, "case {label}: region over the subtree");
    }
}

#[test]
fn damage_filter_paints_leaves_in_any_rect() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (key, x, y) in &[("tl", 0.0, 0.0), ("tr", 160.0, 0.0), ("bl", 0.0, 160.0)] {
                    Block::new()
                        .id(WidgetId::from_hash(*key))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .position(Vec2::new(*x, *y))
                        .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                }
            });
    });
    let rects = [
        Rect::new(0.0, 0.0, 50.0, 50.0),
        Rect::new(150.0, 0.0, 50.0, 50.0),
    ];
    let cmds = h.encode_paint_for(DamageRegion::from_rects(&rects));
    assert_eq!(
        count_draw_rects(&cmds),
        2,
        "two top corners inside damage, bottom corner outside both",
    );
}

#[test]
fn viewport_and_damage_culls_advance_the_sparse_paint_anim_cursor() {
    use crate::display::Display;

    use crate::scene::tree::paint_anims::curves;
    use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
    use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
    use crate::shape::Shape;
    use std::time::Duration;

    const HALF: Duration = Duration::from_millis(500);

    #[derive(Clone, Copy, Debug)]
    enum Cull {
        Viewport,
        Damage,
    }

    for cull in [Cull::Viewport, Cull::Damage] {
        let display = Display::from_physical(UVec2::new(100, 100), 1.0);
        let mut h = UiHarness::new(display.physical);
        h.at(HALF).frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for (key, position, started_at) in [
                        (
                            "culled-visible",
                            match cull {
                                Cull::Viewport => Vec2::new(500.0, 500.0),
                                Cull::Damage => Vec2::new(10.0, 10.0),
                            },
                            HALF,
                        ),
                        ("kept-hidden", Vec2::new(60.0, 10.0), Duration::ZERO),
                    ] {
                        Panel::zstack()
                            .id(WidgetId::from_hash(key))
                            .position(position)
                            .size(20.0)
                            .show(ui, |ui| {
                                ui.add_shape_animated(
                                    Shape::rect(Rect::new(0.0, 0.0, 20.0, 20.0))
                                        .fill(RgbaF32::WHITE),
                                    PaintAnimation::alpha(0.0, 1.0)
                                        .with_started_at(started_at)
                                        .with_period(HALF * 2)
                                        .with_steps(2)
                                        .with_repeat(PaintRepeat::Settle(Duration::MAX))
                                        .with_curve(curves::square),
                                );
                            });
                    }
                });
        });

        let animated: Vec<u32> =
            h.ui.tree(Layer::Main)
                .paint_anims
                .entries
                .iter()
                .map(|entry| entry.shape_idx)
                .collect();
        assert_eq!(animated, [0, 1]);
        let cmds = match cull {
            Cull::Viewport => h.encode_paint(),
            Cull::Damage => {
                h.encode_paint_for(DamageRegion::from(Rect::new(55.0, 5.0, 35.0, 30.0)))
            }
        };
        assert_eq!(
            count_draw_rects(&cmds),
            0,
            "{cull:?}: the first visible animation must be culled and the later hidden animation must still be sampled",
        );
        // The same scene uncut by damage draws the visible one, so the
        // zero above is the cull's and not an encoder that draws nothing.
        if let Cull::Damage = cull {
            assert_eq!(
                count_draw_rects(&h.encode_paint()),
                1,
                "uncut, the visible one draws"
            );
        }
    }
}

/// Soundness repro for the encoder's damage cull, which must test
/// `LayerCascade::subtree_paint_rects` (the node's own extent rolled up
/// with every descendant's) rather than the node's own paint extent
/// alone. When a descendant overflows the parent — a Canvas-positioned
/// child placed outside the parent's `Fixed` bound, a shape with
/// negative-margin overhang — an own-extent test at the parent skips the
/// whole subtree even though a descendant's pixels DO lie inside damage.
/// Symptom in real apps: panning a `Scroll` over a node-graph leaves
/// trails of stale curves because the cull misses overhanging
/// port-circle children.
///
/// This test forces that exact shape:
///   parent (Canvas, Fixed 50×50) at (0..50, 0..50)
///   └── child (Frame, Fixed 40×40) `.position(60, 0)` → (60..100, 0..40)
/// Damage = (60..100, 0..40) — exactly the child's rect, no overlap
/// with the parent's own rect. The child MUST emit a rect quad.
#[test]
fn damage_filter_includes_descendant_overflowing_parent_rect() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("overflow-parent"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("overflowing-child"))
                        .position((60.0, 0.0))
                        .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                        .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                });
        });
    });
    let damage = Rect::new(60.0, 0.0, 40.0, 40.0);
    let cmds = h.encode_paint_for(DamageRegion::from(damage));
    assert_eq!(
        count_draw_rects(&cmds),
        1,
        "the overflowing child paints inside damage and must not be culled by the parent's tight `paint_rect`",
    );
}

/// Regression: a static node sitting in the backend's AA-padding ring —
/// just *outside* the raw damage rect but inside the
/// `RenderPlan::AA_PADDING`
/// (2 physical px) the backend PreClears around each scissor — must still
/// emit its draw. The backend clears the padded region every partial
/// frame; if the encoder's subtree-cull only tested the raw (unpadded)
/// damage rect, that node would be cleared but never repainted, leaving a
/// hard cut exactly along the damage boundary. This is the "dragging a
/// bezier wire past a node border / port circle leaves it cropped along
/// the wire's bbox edge" bug.
///
/// A node comfortably *beyond* the pad ring must still be culled, so the
/// margin doesn't silently disable damage culling.
#[test]
fn damage_filter_repaints_neighbor_in_aa_pad_ring() {
    // At `scale_factor() == 1` (UiHarness::new) the cull margin is
    // `RenderPlan::AA_PADDING + 1 = 3` logical px. A neighbor 2 px away is
    // inside the pad the backend clears → must repaint; one 10 px away is
    // well past the margin → must stay culled.
    let cases: &[(&str, Rect, usize)] = &[
        ("within_aa_pad_gap_2", Rect::new(60.0, 100.0, 38.0, 20.0), 1),
        // The margin repaints any gap under 3 px. At 3 the pad's 2 px of
        // clear stops a pixel short of the neighbour, so it stays culled.
        ("at_the_margin_gap_3", Rect::new(60.0, 100.0, 37.0, 20.0), 0),
        (
            "past_the_margin_gap_4",
            Rect::new(60.0, 100.0, 36.0, 20.0),
            0,
        ),
        ("beyond_pad_gap_10", Rect::new(60.0, 100.0, 30.0, 20.0), 0),
    ];
    for (label, damage, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Static neighbour at (100..120, 100..120) — stands in
                    // for a node border / port circle the wire swept past.
                    Block::new()
                        .id(WidgetId::from_hash("neighbour"))
                        .position(Vec2::new(100.0, 100.0))
                        .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                        .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                        .show(ui);
                });
        });
        let cmds = h.encode_paint_for(DamageRegion::from(*damage));
        assert_eq!(count_draw_rects(&cmds), *expected, "case: {label}");
    }
}
