//! Shapes under a transformed ancestor, and the bounds they claim.

use crate::host::harness::Harness;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::encoder::tests::support::{rect_with_fill, screen_rects_by_fill};
use crate::renderer::frontend::payload::draw_stroke_payload::StrokePath;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// A turned stroke's payload carries its turn about the owner's center in the encoder's logical
/// frame, as every payload does, and its geometry owner-local and unturned: the composer turns it.
/// The owner, 80 × 40, sits 30 px right of a spacer, so its center is (70, 20), where the
/// owner-local frame would read (40, 20). The polyline keeps its points' box (10, 10)..(70, 30),
/// and the arc its basis.
#[test]
fn a_turned_strokes_payload_carries_its_pivot_in_the_logical_frame() {
    use crate::internals::paint_capture::PaintCall;
    use crate::shape::Shape;
    use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
    use std::f32::consts::{PI, TAU};
    use std::time::Duration;

    #[derive(Clone, Copy, Debug)]
    enum Spun {
        Polyline,
        Arc,
    }
    let arc = CircularArc {
        center: Vec2::new(50.0, 20.0),
        radius: 10.0,
        start_angle: 0.0,
        sweep: PI,
    };
    let center = Vec2::new(70.0, 20.0);
    for spun in [Spun::Polyline, Spun::Arc] {
        let mut h = Harness::mono(UVec2::new(200, 200));
        // 1 s at 1 rad/s samples a rotation of 1 rad, so the encoder takes the spin branch.
        h.at(Duration::from_secs(1)).frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("spacer"))
                    .size((Sizing::fixed(30.0), Sizing::fixed(40.0)))
                    .show(ui);
                Panel::zstack()
                    .id(WidgetId::from_hash("spin_owner"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        let turn = PaintAnimation::turn(0.0, 1.0)
                            .with_started_at(Duration::ZERO)
                            .with_period(Duration::from_secs_f32(TAU / 1.0))
                            .with_repeat(PaintRepeat::Forever);
                        let shape = match spun {
                            Spun::Polyline => ui.add_shape(
                                Shape::polyline(&[Vec2::new(10.0, 10.0), Vec2::new(70.0, 30.0)]),
                                Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                            ),
                            Spun::Arc => ui.add_shape(
                                Shape::arc(arc.start_angle, arc.sweep).at(Rect::new(
                                    arc.center.x - arc.radius,
                                    arc.center.y - arc.radius,
                                    2.0 * arc.radius,
                                    2.0 * arc.radius,
                                )),
                                Stroke::new(RgbaF32::WHITE, 2.0),
                            ),
                        };
                        shape.expect("the stroke paints").animate(turn);
                    });
            });
        });
        let cmds = h.encode_paint();
        let spin = match (spun, cmds.calls.as_slice()) {
            (Spun::Polyline, [PaintCall::Stroke(p)]) => {
                let StrokePath::Polyline(path) = p.path else {
                    panic!("a polyline strokes a polyline, got {:?}", p.path);
                };
                assert_eq!(
                    path.bbox,
                    Rect::new(10.0, 10.0, 60.0, 20.0),
                    "owner-local box"
                );
                assert_eq!(p.origin, Vec2::new(30.0, 0.0));
                p.spin
            }
            (Spun::Arc, [PaintCall::Stroke(p)]) => {
                assert_eq!(
                    p.path,
                    StrokePath::Curve(CurveBasis::Arc(arc)),
                    "the basis stays owner-local"
                );
                assert_eq!(p.origin, Vec2::new(30.0, 0.0));
                p.spin
            }
            _ => panic!("{spun:?}: expected one draw, got {:?}", cmds.kinds()),
        };
        let spin = spin.expect("the turn samples a non-zero rotation");
        assert_eq!(spin.pivot, center, "{spun:?}");
        assert_eq!(spin.angle, 1.0, "{spun:?}");
    }
}

/// `Panel::transform` applies to the panel's body (direct shapes and child subtrees), not its chrome.
#[test]
fn transformed_panel_applies_transform_to_direct_shapes() {
    use crate::shape::Shape;

    let shape_color = RgbaF32::srgb(0.2, 0.6, 0.9);
    let child_color = RgbaF32::srgb(0.9, 0.4, 0.2);
    let scale = 2.0;
    let transform = TranslateScale::new(Vec2::new(10.0, 20.0), scale);

    // Shape 30×30 at (0, 0), child 40×40 at (50, 60); under `transform` the screen rects are:
    //   shape: min = (10, 20), size = (60, 60)
    //   child: min = (10 + 50*2, 20 + 60*2) = (110, 140), size = (80, 80)
    let mut h = Harness::mono(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(300.0))
                .transform(transform)
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect()
                            .at(Rect::new(0.0, 0.0, 30.0, 30.0))
                            .corners(0.0),
                        shape_color,
                    );
                    Block::new()
                        .id(WidgetId::from_hash("child"))
                        .at((50.0, 60.0))
                        .size(40.0)
                        .chrome(Chrome::new(child_color))
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

/// Chrome paints in parent space: the chrome still frames the viewport while the body pans or zooms.
#[test]
fn transformed_panel_chrome_stays_in_parent_space() {
    let chrome_color = RgbaF32::srgb(0.1, 0.1, 0.1);
    let transform = TranslateScale::new(Vec2::new(50.0, 50.0), 2.0);

    let mut h = Harness::mono(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::fixed(150.0))
                .transform(transform)
                .chrome(Chrome::new(chrome_color))
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
