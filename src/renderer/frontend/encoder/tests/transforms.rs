//! Shapes under a transformed ancestor, and the bounds they claim.

use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::encoder::tests::support::{rect_with_fill, screen_rects_by_fill};
use crate::shape::paint::curve_basis::CurveBasis;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// A spun shape's payload bounds are the rotation-invariant square about the owner-box centre
/// c = (40, 20) (box 80×40) holding the swept centerline bbox; the pivot is carried, not inferred.
/// Polyline r = √(30² + 10²) ≈ 31.6228; arc bbox (40,20)..(60,30), r = √(20² + 10²). Arc lanes stay
/// owner-local and unrotated; the composer spins them.
#[test]
fn spun_shape_bounds_are_rotation_invariant_squares_about_owner_centre() {
    use crate::display::Display;
    use crate::internals::paint_capture::PaintCall;
    use crate::scene::tree::paint_anims::curves;
    use crate::scene::tree::paint_anims::paint_animation::{PaintAnimation, PaintRepeat};
    use crate::shape::Shape;
    use std::f32::consts::{PI, TAU};
    use std::time::Duration;

    #[derive(Clone, Copy, Debug)]
    enum Spun {
        Polyline,
        Arc,
    }
    let arc_basis = CurveBasis::Arc {
        center: Vec2::new(50.0, 20.0),
        radius: 10.0,
        a0: 0.0,
        a1: PI,
    };
    let c = Vec2::new(40.0, 20.0);
    for (spun, half_extent) in [
        (Spun::Polyline, 30.0_f32.hypot(10.0)),
        (Spun::Arc, 20.0_f32.hypot(10.0)),
    ] {
        let display = Display::from_physical(UVec2::new(200, 200), 1.0);
        let mut h = UiHarness::new(display.physical);
        // 1 s at 1 rad/s samples a rotation of 1 rad, so the encoder takes the spin branch.
        h.at(Duration::from_secs(1)).frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("spin_owner"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        let turn = PaintAnimation::turn(0.0, 1.0)
                            .with_started_at(Duration::ZERO)
                            .with_period(Duration::from_secs_f32(TAU / 1.0))
                            .with_repeat(PaintRepeat::Forever)
                            .with_curve(curves::linear);
                        match spun {
                            Spun::Polyline => ui.add_shape_animated(
                                Shape::polyline(
                                    &[Vec2::new(10.0, 10.0), Vec2::new(70.0, 30.0)],
                                    Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                                ),
                                turn,
                            ),
                            Spun::Arc => ui.add_shape_animated(
                                Shape::arc(
                                    Vec2::new(50.0, 20.0),
                                    10.0,
                                    0.0,
                                    PI,
                                    Stroke::new(RgbaF32::WHITE, 2.0),
                                ),
                                turn,
                            ),
                        }
                    });
            });
        });
        let cmds = h.encode_paint();
        let bounds = match (spun, cmds.calls.as_slice()) {
            (Spun::Polyline, [PaintCall::Polyline(p)]) => p.bounds,
            (Spun::Arc, [PaintCall::Curve(p)]) => {
                assert_eq!(p.basis, arc_basis, "the lanes stay owner-local");
                p.bounds
            }
            _ => panic!("{spun:?}: expected one draw, got {:?}", cmds.kinds()),
        };
        let spin = bounds.spin().expect("spin must sample a non-zero rotation");
        assert_eq!(spin.pivot, c, "{spun:?} pivot {:?}", spin.pivot);
        assert!(spin.angle != 0.0, "{spun:?}");
        let cull = bounds.cull_rect();
        let square = Rect::new(
            c.x - half_extent,
            c.y - half_extent,
            2.0 * half_extent,
            2.0 * half_extent,
        );
        assert_eq!(cull, square, "{spun:?}");
    }
    // The polyline's far endpoint rotated 90° about c stays inside its
    // square, and outside the owner box.
    let p_rot = c + Vec2::new(-10.0, 30.0);
    let r = 30.0_f32.hypot(10.0);
    assert!(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r).contains(p_rot));
    assert!(!Rect::new(0.0, 0.0, 80.0, 40.0).contains(p_rot));
}

/// `Panel::transform` applies to the panel's body (direct shapes and child subtrees), not its chrome.
#[test]
fn transformed_panel_applies_transform_to_direct_shapes() {
    use crate::shape::Shape;

    let shape_color = RgbaF32::srgb(0.2, 0.6, 0.9);
    let child_color = RgbaF32::srgb(0.9, 0.4, 0.2);
    let scale = 2.0;
    let xform = TranslateScale::new(Vec2::new(10.0, 20.0), scale);

    // Shape 30×30 at (0, 0), child 40×40 at (50, 60); under `xform` the screen rects are:
    //   shape: min = (10, 20), size = (60, 60)
    //   child: min = (10 + 50*2, 20 + 60*2) = (110, 140), size = (80, 80)
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
                            .corners(0.0)
                            .fill(shape_color),
                    );
                    Block::new()
                        .id(WidgetId::from_hash("child"))
                        .position((50.0, 60.0))
                        .size(40.0)
                        .background(Background::fill(child_color))
                        .show(ui);
                });
        });
    });

    let drawn = screen_rects_by_fill(&h.encode_paint());
    let shape_rect = rect_with_fill(&drawn, shape_color).expect("direct shape must paint");
    let child_rect = rect_with_fill(&drawn, child_color).expect("child must paint");

    assert_eq!(shape_rect, Rect::new(10.0, 20.0, 60.0, 60.0));
    assert_eq!(child_rect, Rect::new(110.0, 140.0, 80.0, 80.0));
}

/// Chrome paints in parent space: a background still frames the viewport while the body pans or zooms.
#[test]
fn transformed_panel_chrome_stays_in_parent_space() {
    let chrome_color = RgbaF32::srgb(0.1, 0.1, 0.1);
    let xform = TranslateScale::new(Vec2::new(50.0, 50.0), 2.0);

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(150.0))
                .transform(xform)
                .background(Background::fill(chrome_color))
                .show(ui, |_| {});
        });
    });

    let drawn = screen_rects_by_fill(&h.encode_paint());
    let chrome_rect = rect_with_fill(&drawn, chrome_color).expect("chrome must paint");

    // Chrome paints at the panel's own layout rect; the transform must not scale it.
    assert_eq!(
        chrome_rect,
        Rect::new(0.0, 0.0, 150.0, 150.0),
        "chrome is not scaled by its own transform",
    );
}
