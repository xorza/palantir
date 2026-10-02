//! Shapes under a transformed ancestor, and the bounds they claim.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect, translate_scale::TranslateScale};
use crate::renderer::frontend::encoder::tests::support::{rect_with_fill, screen_rects_by_fill};
use crate::scene::shapes::paint::curve_basis::CurveBasis;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// A spun shape's payload bounds must be rotation-invariant: the
/// smallest square centred on the owner-box centre that holds the
/// centerline bbox swept about it, with the pivot carried rather than
/// inferred. The composer applies stroke reach after this sweep. Owner
/// box 80×40 → pivot c = (40, 20).
///
/// - **Polyline** (10,10)..(70,30): max corner distance from c is
///   dx = 30, dy = 10, r = √(30² + 10²) = √1000 ≈ 31.6228. The far
///   endpoint (70,30) rotated 90° CCW about c — c + (−10,30) = (30,50)
///   — lies outside the owner box but inside the square: an owner-box
///   bound is not rotation-safe.
/// - **Arc** about (50,20), radius 10, 0..π: the centerline bbox spans
///   (40,20)..(60,30) — endpoints and the π/2 crossing — so
///   r = √(20² + 10²). Its geometry lanes ride owner-local and
///   unrotated on the arc basis; the composer spins them at compose
///   time, so both ends of the pivot contract meet here.
#[test]
fn spun_shape_bounds_are_rotation_invariant_squares_about_owner_centre() {
    use crate::display::Display;
    use crate::renderer::frontend::capture::PaintCall;
    use crate::scene::tree::paint_anims::curves;
    use crate::scene::tree::paint_anims::paint_anim::{PaintAnim, PaintRepeat};
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
        (Spun::Polyline, (30.0_f32 * 30.0 + 10.0 * 10.0).sqrt()),
        (Spun::Arc, (20.0_f32 * 20.0 + 10.0 * 10.0).sqrt()),
    ] {
        let display = Display::from_physical(UVec2::new(200, 200), 1.0);
        let mut h = UiHarness::new(display.physical);
        // 1 s in at 1 rad/s → sampled rotation = 1 rad ≠ 0, so the
        // encoder takes the spin branch.
        h.at(Duration::from_secs(1)).frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("spin_owner"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        let turn = PaintAnim::turn(0.0, 1.0)
                            .started_at(Duration::ZERO)
                            .period(Duration::from_secs_f32(TAU / 1.0))
                            .repeat(PaintRepeat::Forever)
                            .curve(curves::linear);
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
        // The pivot is carried, not inferred from the cull rect's centre.
        assert_eq!(spin.pivot, c, "{spun:?} pivot {:?}", spin.pivot);
        assert!(spin.angle != 0.0, "{spun:?}");
        // The cull rect is the rotation-invariant square about it, so the
        // composer's overlap tracking holds at every angle.
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
    let r = (30.0_f32 * 30.0 + 10.0 * 10.0).sqrt();
    assert!(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r).contains(p_rot));
    assert!(!Rect::new(0.0, 0.0, 80.0, 40.0).contains(p_rot));
}

/// `Panel::transform` applies to the panel's body — both direct
/// shapes (recorded via `ui.add_shape`) and child subtrees. Pins the
/// "shapes inside the panel's transform" contract; the inverse case
/// (chrome stays in parent space) is covered by
/// `transformed_panel_chrome_stays_in_parent_space` below.
#[test]
fn transformed_panel_applies_transform_to_direct_shapes() {
    use crate::shape::Shape;

    let shape_color = RgbaF32::srgb(0.2, 0.6, 0.9);
    let child_color = RgbaF32::srgb(0.9, 0.4, 0.2);
    let scale = 2.0;
    let xform = TranslateScale::new(Vec2::new(10.0, 20.0), scale);

    // Shape is 30×30 at panel-local (0, 0); child is 40×40 at
    // panel-local (50, 60). Under `xform`, screen rects should be:
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

/// Chrome on a transformed panel paints in parent space (unaffected
/// by the panel's own transform), so a panel's background still
/// frames the viewport while its body pans/zooms underneath. The
/// flip side of `transformed_panel_applies_transform_to_direct_shapes`.
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

    // Chrome paints at the panel's own layout rect (Sizing::fixed(150.0)
    // inside a 400×400 surface, hstack with one child → top-left at (0,0)
    // by default). The transform must NOT scale chrome to 300×300.
    assert_eq!(
        chrome_rect,
        Rect::new(0.0, 0.0, 150.0, 150.0),
        "chrome is not scaled by its own transform",
    );
}
