//! How an image or icon resolves its destination rect and UVs.

use crate::damage::region::DamageRegion;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::renderer::frontend::payload::draw_stroke_payload::{DrawStrokePayload, StrokePath};
use crate::shape::image_paint::ImagePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// Each [`ImageDownsample`] mode reaches the shader as its own flag bit, `Single` as none. The record is gone by the fragment stage, so a zero encoding would draw as the default and a shared bit as another mode; tap bits must stay clear of the filter bits in the same `flags` word.
#[test]
fn downsample_modes_encode_to_distinct_tap_flags() {
    use crate::primitives::paint::image::{Image, ImageDownsample};
    use crate::renderer::render_buffer::image_flags::ImageFlags;
    use crate::shape::Shape;

    let modes = [
        ("Single", ImageDownsample::Single, ImageFlags::NONE),
        ("Mean", ImageDownsample::Mean, ImageFlags::TAPS_MEAN),
        ("Peak", ImageDownsample::Peak, ImageFlags::TAPS_PEAK),
    ];

    let mut h = Harness::mono(UVec2::new(200, 200));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(2, 2), vec![255; 16]).unwrap())
        .unwrap();
    // Three shapes on one node paint the same rect; record order pairs each draw with its mode.
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (_, mode, _) in modes {
                    ui.add_shape(
                        Shape::rect(),
                        ImagePaint::new(handle.clone()).downsample(mode),
                    );
                }
            });
    });

    let cmds = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 200.0, 200.0)));
    assert_eq!(cmds.kinds(), ["Image"; 3], "one image draw per mode");
    let flags = cmds
        .calls
        .iter()
        .map(|call| call.as_image().unwrap().paint.flags);
    for ((label, _, expected), actual) in modes.into_iter().zip(flags) {
        assert_eq!(actual, expected, "{label} encoded the wrong tap flags");
        let filters = ImageFlags::MIN_NEAREST.union(ImageFlags::MAG_NEAREST);
        assert_eq!(
            actual.bits() & filters.bits(),
            0,
            "{label} must not collide with the filter bits",
        );
    }
}

/// An image's payload carries the shape that clips it: a rect's, the one the image fits in, with
/// its radii, square ones too. A contained 2×2 image in (0, 0, 100 × 50) is (25, 0, 50 × 50). A
/// sharp triangle (0, 0), (40, 0),
/// (0, 20) fits its image in its covering rect, (0, 0, 40 × 20), so contained at (10, 0, 20 × 20),
/// and carries its corners. A circle in the same rect fits its image in its square, (25, 0,
/// 50 × 50), so a filled image takes that square, and carries it rounded by 25.
#[test]
fn an_image_carries_the_shape_that_clips_it() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::image::{Image, ImageFit};
    use crate::shape::Shape;
    use crate::shape::geometry::box_geometry::PlacedBox;
    use crate::shape::geometry::closed_geometry::PlacedClosed;
    use crate::shape::geometry::triangle_geometry::PlacedTriangle;

    let mut h = Harness::mono(UVec2::new(200, 200));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(2, 2), vec![255; 16]).unwrap())
        .unwrap();
    let at = Rect::new(0.0, 0.0, 100.0, 50.0);
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for corners in [Corners::all(6.0), Corners::ZERO] {
                    ui.add_shape(
                        Shape::rect().at(at).corners(corners),
                        ImagePaint::new(handle.clone()).fit(ImageFit::Contain),
                    );
                }
                ui.add_shape(
                    Shape::triangle(Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(0.0, 20.0)),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Contain),
                );
                ui.add_shape(
                    Shape::circle().at(at),
                    ImagePaint::new(handle.clone()).fit(ImageFit::Fill),
                );
            });
    });

    let cmds = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 200.0, 200.0)));
    assert_eq!(cmds.kinds(), ["Image"; 4]);
    let images: Vec<_> = cmds
        .calls
        .iter()
        .map(|call| call.as_image().unwrap())
        .collect();
    for image in &images[..2] {
        assert_eq!(image.paint.rect, Rect::new(25.0, 0.0, 50.0, 50.0));
    }
    assert_eq!(
        images[0].geometry,
        PlacedClosed::inside(PlacedBox {
            rect: at,
            corners: Corners::all(6.0),
        })
    );
    assert_eq!(
        images[1].geometry,
        PlacedClosed::inside(PlacedBox {
            rect: at,
            corners: Corners::ZERO,
        })
    );
    assert_eq!(images[2].paint.rect, Rect::new(10.0, 0.0, 20.0, 20.0));
    assert_eq!(
        images[2].geometry,
        PlacedClosed::Triangle(PlacedTriangle {
            centers: [Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(0.0, 20.0)],
            radius: 0.0,
        })
    );
    let square = Rect::new(25.0, 0.0, 50.0, 50.0);
    assert_eq!(images[3].paint.rect, square);
    assert_eq!(
        images[3].geometry,
        PlacedClosed::inside(PlacedBox {
            rect: square,
            corners: Corners::all(25.0),
        })
    );
}

/// The cascade bounds an image by the rect the encoder draws under every fit, so an `ImageFit::Natural` image overflowing its node is damaged and culled where it paints. A 200×100 image in a 100×100 node at (40, 40), no transform or clip.
#[test]
fn the_cascade_bounds_an_image_by_the_rect_the_encoder_draws() {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::paint::image::{Image, ImageFit};
    use crate::shape::Shape;

    let fits = [
        ImageFit::Fill,
        ImageFit::Contain,
        ImageFit::Cover,
        ImageFit::Natural,
        ImageFit::Tile {
            offset: Vec2::ZERO,
            scale: Vec2::splat(2.0),
        },
    ];
    // Hand-computed: Contain scales min(100/200, 100/100) = 0.5 → 100×50, centered at y = 40 + 25; None paints 200×100 centered, x = 40 + (100 - 200)/2 = -10, y = 40.
    let node = Rect::new(40.0, 40.0, 100.0, 100.0);
    let drawn = [
        node,
        Rect::new(40.0, 65.0, 100.0, 50.0),
        node,
        Rect::new(-10.0, 40.0, 200.0, 100.0),
        node,
    ];
    let mut h = Harness::mono(UVec2::new(300, 300));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(200, 100), vec![255; 200 * 100 * 4]).unwrap())
        .unwrap();
    for (fit, expected) in fits.into_iter().zip(drawn) {
        h.frame(|ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("frame"))
                        .at((node.min.x, node.min.y))
                        .size(node.size.w)
                        .show(ui, |ui| {
                            ui.add_shape(Shape::rect(), ImagePaint::new(handle.clone()).fit(fit));
                        });
                });
        });
        let cmds = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 300.0, 300.0)));
        assert_eq!(cmds.kinds(), ["Image"], "{fit:?}");
        assert_eq!(
            cmds.calls[0].as_image().unwrap().paint.rect,
            expected,
            "{fit:?} draw"
        );
        let mut rows = Vec::new();
        h.ui().cascade().owned_paints(h.ui().forest(), &mut rows);
        let screens: Vec<Rect> = rows
            .iter()
            .filter(|row| row.owner == WidgetId::from_hash("frame"))
            .map(|row| row.screen)
            .collect();
        assert_eq!(screens, [expected], "{fit:?} cascade bound");
    }
}

/// A point shape fits its owner where it is drawn: the polyline (0, 0)–(10, 10) under
/// `Fit::Fill` in a 40 × 20 canvas scales by (40 / 10, 20 / 10) = (4, 2) about the centers,
/// offset `(20, 10) − (5, 5) · (4, 2) = (0, 0)`, and the payload's bound is the fitted extent.
#[test]
fn a_polyline_fits_its_owner_where_it_is_drawn() {
    use crate::internals::paint_capture::PaintCall;
    use crate::primitives::geometry::fit::{Fit, FitMap};
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::Shape;

    let mut h = Harness::mono(UVec2::new(100, 100));
    let points = [Vec2::ZERO, Vec2::splat(10.0)];
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::polyline(&points).fit(Fit::Fill),
                    Stroke::new(RgbaF32::WHITE, 1.0),
                );
            });
    });
    let cmds = h.encode_paint();
    let [
        PaintCall::Stroke(DrawStrokePayload {
            path: StrokePath::Polyline(p),
            ..
        }),
    ] = cmds.calls.as_slice()
    else {
        panic!("one polyline draw, got {:?}", cmds.kinds());
    };
    assert_eq!(
        p.fit,
        FitMap {
            offset: Vec2::ZERO,
            scale: Vec2::new(4.0, 2.0),
        }
    );
    assert_eq!(p.bbox, Rect::new(0.0, 0.0, 40.0, 20.0));
}

/// An arc of no sweep paints its round cap as a disc in the stroke color times its ramp at the
/// start: white times red is red. The 20 × 20 canvas puts the circle's center at (10, 10) and its
/// start, 10 along +x, at (20, 10), so the 4 px dot covers (18, 8)–(22, 12).
#[test]
fn an_arc_of_no_sweep_paints_its_dot_in_the_ramp_start() {
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::line_style::LineCap;
    use crate::primitives::paint::stroke::Stroke;
    use crate::renderer::frontend::encoder::tests::support;
    use crate::shape::Shape;

    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let ramp = ColorRamp::two_stop(red, RgbaF32::new(0.0, 0.0, 1.0, 1.0));
    let mut h = Harness::mono(UVec2::new(100, 100));
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::arc(0.0, 0.0),
                    Stroke::new(RgbaF32::WHITE, 4.0)
                        .with_ramp(ramp)
                        .with_cap(LineCap::Round),
                );
            });
    });
    let cmds = h.encode_paint();
    let rects: Vec<_> = cmds.calls.iter().filter_map(support::as_rect).collect();
    assert_eq!(rects.len(), 1, "one dot, got {:?}", cmds.kinds());
    assert_eq!(rects[0].paint.fill.color, red.to_f16());
    assert_eq!(support::quad_rect(rects[0]), Rect::new(18.0, 8.0, 4.0, 4.0));
}

/// The cascade bounds a triangle's image as it bounds the triangle in a color: the triangle clips
/// the image, whatever its fit. The triangle (0, 0), (40, 0), (0, 20) in a node at (40, 40) covers
/// (40, 40, 40 × 20), grown by half a pixel of antialiasing at scale 1.
#[test]
fn the_cascade_bounds_a_triangles_image_as_its_triangle() {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::image::{Image, ImageFit};
    use crate::shape::Shape;

    let mut h = Harness::mono(UVec2::new(300, 300));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(2, 2), vec![255; 16]).unwrap())
        .unwrap();
    let triangle = || Shape::triangle(Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(0.0, 20.0));
    h.frame(|ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("frame"))
                    .at((40.0, 40.0))
                    .size(100.0)
                    .show(ui, |ui| {
                        ui.add_shape(
                            triangle(),
                            ImagePaint::new(handle.clone()).fit(ImageFit::Natural),
                        );
                        ui.add_shape(triangle(), RgbaF32::WHITE);
                    });
            });
    });
    let mut rows = Vec::new();
    h.ui().cascade().owned_paints(h.ui().forest(), &mut rows);
    let screens: Vec<Rect> = rows
        .iter()
        .filter(|row| row.owner == WidgetId::from_hash("frame"))
        .map(|row| row.screen)
        .collect();
    let bound = Rect::new(39.5, 39.5, 41.0, 21.0);
    assert_eq!(screens, [bound, bound]);
}
