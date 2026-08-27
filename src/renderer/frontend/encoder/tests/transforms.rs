//! Shapes under a transformed ancestor, and the bounds they claim.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, translate_scale::TranslateScale};
use crate::renderer::frontend::capture::PaintCall;
use crate::renderer::frontend::encoder::tests::support::screen_rects_by_fill;
use crate::scene::node::configure::Configure;
use crate::scene::shapes::paint::CurveBasis;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// A spun polyline's payload bbox must be rotation-invariant: the
/// smallest square centred on the owner-box centre (the composer's
/// centerline bbox. Owner box 80×40 → pivot c = (40, 20).
/// Points span (10,10)..(70,30), so max corner distance from c:
/// dx = 30, dy = 10, r = √(30² + 10²) = √1000 ≈ 31.6228.
/// The composer applies miter/cap/fringe reach after this sweep.
/// The far endpoint (70,30) rotated 90° CCW about c —
/// c + rot90(30,10) = c + (−10,30) = (30,50) — lies OUTSIDE the owner
/// box (0,0,80,40) the old code shipped as the bbox, but inside the
/// new square: rotation-safety is exactly what the old bound lacked.
#[test]
fn spun_polyline_bbox_is_rotation_invariant_square_about_owner_centre() {
    use crate::display::Display;
    use crate::scene::tree::paint_anims::PaintAnim;

    use crate::shape::Shape;
    use crate::shape::polyline::PolylineColors;
    use std::time::Duration;

    let display = Display::from_physical(UVec2::new(200, 200), 1.0);
    let mut h = UiHarness::new(display.physical);
    // 1 s in at 1 rad/s → sampled rotation = 1 rad ≠ 0, so the encoder
    // takes the spin branch.
    h.at(Duration::from_secs(1)).frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("spin_owner"))
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    ui.add_shape_animated(
                        Shape::polyline(
                            &[Vec2::new(10.0, 10.0), Vec2::new(70.0, 30.0)],
                            PolylineColors::Single(Color::rgb(1.0, 0.0, 0.0)),
                            1.0,
                        ),
                        PaintAnim::Spin {
                            speed: 1.0,
                            started_at: Duration::ZERO,
                        },
                    );
                });
        });
    });
    let cmds = h.encode_paint();
    let p = cmds
        .calls
        .iter()
        .find_map(|command| match command {
            PaintCall::Polyline(payload) => Some(payload),
            _ => None,
        })
        .expect("spun polyline must emit a DrawPolyline");
    let spin = p
        .bounds
        .spin()
        .expect("spin must sample a non-zero rotation");

    let c = Vec2::new(40.0, 20.0);
    let r = (30.0_f32 * 30.0 + 10.0 * 10.0).sqrt();
    let eps = 1e-3;
    // The pivot is carried, not inferred from the cull rect's centre.
    assert!((spin.pivot - c).length() < eps, "pivot {:?}", spin.pivot);
    assert!(spin.angle != 0.0);
    // The cull rect is still the rotation-invariant square about it, so
    // the composer's overlap tracking holds at every angle.
    let cull = p.bounds.cull_rect();
    assert!((cull.min.x - (c.x - r)).abs() < eps, "cull {cull:?}");
    assert!((cull.min.y - (c.y - r)).abs() < eps, "cull {cull:?}");
    assert!((cull.size.w - 2.0 * r).abs() < eps, "cull {cull:?}");
    assert!((cull.size.h - 2.0 * r).abs() < eps, "cull {cull:?}");
    // The far endpoint rotated 90° about c stays inside it…
    let p_rot = c + Vec2::new(-10.0, 30.0);
    assert!(cull.contains(p_rot), "cull {cull:?} misses {p_rot:?}");
    // …but not inside the owner box the old code used.
    assert!(!Rect::new(0.0, 0.0, 80.0, 40.0).contains(p_rot));
}

/// Same rotation-safety contract for the GPU-arc path: a spun
/// `Shape::arc` ships the rotation-invariant square centred on the
/// owner-box centre plus the sampled rotation, while the geometry
/// lanes stay owner-local and unrotated — the composer applies the
/// spin at compose time (center about `bbox.center()`, angles by
/// `rotation`), so both ends of the pivot contract meet here.
#[test]
fn spun_arc_bbox_is_rotation_invariant_square_about_owner_centre() {
    use crate::display::Display;
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;
    use std::f32::consts::PI;
    use std::time::Duration;

    let display = Display::from_physical(UVec2::new(200, 200), 1.0);
    let mut h = UiHarness::new(display.physical);
    // 1 s in at 1 rad/s → sampled rotation = 1 rad ≠ 0.
    h.at(Duration::from_secs(1)).frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("arc_spin_owner"))
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    ui.add_shape_animated(
                        Shape::arc(Vec2::new(50.0, 20.0), 10.0, 0.0, PI, 2.0).brush(Color::WHITE),
                        PaintAnim::Spin {
                            speed: 1.0,
                            started_at: Duration::ZERO,
                        },
                    );
                });
        });
    });
    let cmds = h.encode_paint();
    let p = cmds
        .calls
        .iter()
        .find_map(|command| match command {
            PaintCall::Curve(payload) => Some(payload),
            _ => None,
        })
        .expect("spun arc must emit a curve draw");
    let spin = p
        .bounds
        .spin()
        .expect("spin must sample a non-zero rotation");
    // Geometry rides owner-local and unrotated, on the arc basis.
    assert_eq!(
        p.basis,
        CurveBasis::Arc {
            center: Vec2::new(50.0, 20.0),
            radius: 10.0,
            a0: 0.0,
            a1: PI,
        },
    );

    // Centerline bbox spans (40,20)..(60,30) (endpoints + the π/2
    // crossing). Sweeping it about owner centre c = (40, 20) gives
    // half-extent √(20² + 10²); composer applies stroke reach later.
    let c = Vec2::new(40.0, 20.0);
    let r = (20.0_f32 * 20.0 + 10.0 * 10.0).sqrt();
    let eps = 1e-3;
    let cull = p.bounds.cull_rect();
    assert!((cull.min.x - (c.x - r)).abs() < eps, "cull {cull:?}");
    assert!((cull.min.y - (c.y - r)).abs() < eps, "cull {cull:?}");
    assert!((cull.size.w - 2.0 * r).abs() < eps, "cull {cull:?}");
    assert!((cull.size.h - 2.0 * r).abs() < eps, "cull {cull:?}");
    assert!((spin.pivot - c).length() < eps, "pivot {:?}", spin.pivot);
}

/// `Panel::transform` applies to the panel's body — both direct
/// shapes (recorded via `ui.add_shape`) and child subtrees. Pins the
/// "shapes inside the panel's transform" contract; the inverse case
/// (chrome stays in parent space) is covered by
/// `transformed_panel_chrome_stays_in_parent_space` below.
#[test]
fn transformed_panel_applies_transform_to_direct_shapes() {
    use crate::shape::Shape;

    let shape_color = Color::rgb(0.2, 0.6, 0.9);
    let child_color = Color::rgb(0.9, 0.4, 0.2);
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
                    Frame::new()
                        .id(WidgetId::from_hash("child"))
                        .position((50.0, 60.0))
                        .size(40.0)
                        .background(Background {
                            fill: child_color.into(),
                            ..Default::default()
                        })
                        .show(ui);
                });
        });
    });

    use crate::primitives::color::ColorF16;
    let drawn = screen_rects_by_fill(&h.encode_paint());
    let shape_f16: ColorF16 = shape_color.into();
    let child_f16: ColorF16 = child_color.into();

    let (_, shape_rect) = drawn
        .iter()
        .find(|(c, _)| *c == shape_f16)
        .expect("direct shape must paint");
    let (_, child_rect) = drawn
        .iter()
        .find(|(c, _)| *c == child_f16)
        .expect("child must paint");

    // Composer rounds rects in 8 decimal places (or so) — accept tiny FP drift.
    fn approx_eq(a: Rect, b: Rect) {
        let eps = 1e-3;
        assert!(
            (a.min.x - b.min.x).abs() < eps
                && (a.min.y - b.min.y).abs() < eps
                && (a.size.w - b.size.w).abs() < eps
                && (a.size.h - b.size.h).abs() < eps,
            "expected {a:?}, got {b:?}",
        );
    }
    approx_eq(Rect::new(10.0, 20.0, 60.0, 60.0), *shape_rect);
    approx_eq(Rect::new(110.0, 140.0, 80.0, 80.0), *child_rect);
}

/// Chrome on a transformed panel paints in parent space (unaffected
/// by the panel's own transform), so a panel's background still
/// frames the viewport while its body pans/zooms underneath. The
/// flip side of `transformed_panel_applies_transform_to_direct_shapes`.
#[test]
fn transformed_panel_chrome_stays_in_parent_space() {
    let chrome_color = Color::rgb(0.1, 0.1, 0.1);
    let xform = TranslateScale::new(Vec2::new(50.0, 50.0), 2.0);

    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(150.0))
                .transform(xform)
                .background(Background {
                    fill: chrome_color.into(),
                    ..Default::default()
                })
                .show(ui, |_| {});
        });
    });

    use crate::primitives::color::ColorF16;
    let drawn = screen_rects_by_fill(&h.encode_paint());
    let chrome_f16: ColorF16 = chrome_color.into();
    let (_, chrome_rect) = drawn
        .iter()
        .find(|(c, _)| *c == chrome_f16)
        .expect("chrome must paint");

    // Chrome paints at the panel's own layout rect (Sizing::fixed(150.0)
    // inside a 400×400 surface, hstack with one child → top-left at (0,0)
    // by default). The transform must NOT scale chrome to 300×300.
    assert!(
        (chrome_rect.size.w - 150.0).abs() < 1e-3,
        "chrome width must not be scaled by self transform: got {:?}",
        chrome_rect
    );
    assert!(
        (chrome_rect.size.h - 150.0).abs() < 1e-3,
        "chrome height must not be scaled by self transform: got {:?}",
        chrome_rect
    );
}
