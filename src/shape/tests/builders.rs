use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::shadow::ShadowKind;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::image_paint::ImagePaint;
use crate::shape::internals;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::polyline::COLOR_PRODUCT_RULE;
use crate::shape::sealed::DrawWith;
use crate::text::glyph_font::GlyphFont;
use glam::{UVec2, Vec2};

/// Inputs a builder checks never reach the record gate: each panics with its
/// kind's rule on entry. Gradient geometry most of all, as it interns behind a
/// `GradientId` the gate cannot see.
#[test]
fn builders_refuse_what_they_check() {
    const N: f32 = f32::NAN;
    let ok_rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let white = RgbaF32::WHITE;
    for radius in [N, -1.0, 1.0e5] {
        panic_probe::assert_panics_with(<Corners as LaneCodec>::LANE_RULE, || {
            check(&(Shape::rect().at(ok_rect).corners(radius), white));
        });
    }
    panic_probe::assert_panics_with(domain::COLOR_RULE, || {
        check(&(
            Shape::rect().at(ok_rect),
            Border::new(RgbaF32::srgba(0.0, N, 0.0, 1.0), 2.0),
        ));
    });
    // 65520 is the least magnitude that packs to an infinite f16 lane; 65504 is the greatest
    // finite one. Every value below packs into such a lane, so its kind's rule refuses a NaN or a
    // sign, and the lane rule a magnitude.
    let wide = 65520.0;
    for (rule, radius) in [
        (domain::LENGTH_RULE, N),
        (domain::LENGTH_RULE, -1.0),
        (domain::LANE_RULE, wide),
    ] {
        panic_probe::assert_panics_with(rule, || {
            check(&(
                Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y).radius(radius),
                RgbaF32::WHITE,
            ));
        });
    }
    panic_probe::assert_panics_with(domain::ANGLE_RULE, || {
        check(&(
            Shape::rect().at(ok_rect),
            Gradient::linear(N, [white, RgbaF32::BLACK]),
        ));
    });
    let half = Vec2::splat(0.5);
    let radial = |center, radius| Gradient::radial(center, radius, [white, RgbaF32::BLACK]);
    for (center, radius) in [(Vec2::new(wide, 0.5), half), (half, Vec2::new(0.5, wide))] {
        panic_probe::assert_panics_with(domain::LANE_RULE, || {
            check(&(Shape::rect().at(ok_rect), radial(center, radius)));
        });
    }
    panic_probe::assert_panics_with(domain::LANE_RULE, || {
        check(&(
            Shape::rect().at(ok_rect),
            Gradient::conic(Vec2::new(0.5, -wide), 0.0, [white, RgbaF32::BLACK]),
        ));
    });
    check(&(
        Shape::rect().at(ok_rect),
        radial(Vec2::splat(-65504.0), Vec2::splat(65504.0)),
    ));
    for filter in [
        Material::blur(wide),
        Material::blur(1.0).with_max_luminance(wide),
    ] {
        panic_probe::assert_panics_with(domain::LANE_RULE, || {
            check(&(Shape::rect().at(ok_rect), filter));
        });
    }
    for (rule, shadow) in [
        (
            domain::LENGTH_RULE,
            Shadow {
                blur: N,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_RULE,
            Shadow {
                blur: wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_RULE,
            Shadow {
                spread: wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_RULE,
            Shadow {
                spread: -wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_RULE,
            Shadow {
                offset: Vec2::new(wide, 0.0),
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_RULE,
            Shadow {
                offset: Vec2::new(0.0, -wide),
                ..Shadow::default()
            },
        ),
    ] {
        panic_probe::assert_panics_with(rule, || {
            check(&(
                Shape::rect(),
                Shadow {
                    color: white,
                    ..shadow
                },
            ));
        });
    }
    let widest = Shadow {
        color: white,
        offset: Vec2::splat(-65504.0),
        blur: 65504.0,
        spread: 65504.0,
        kind: ShadowKind::Drop,
    };
    let lowered = LoweredShadow::lower(widest).geometry();
    assert_eq!(
        (lowered.offset, lowered.blur, lowered.spread),
        (widest.offset, widest.blur, widest.spread),
        "the widest shadow packs exactly"
    );
    check(&(Shape::rect(), widest));
    for (start_angle, sweep) in [(N, 1.0), (0.0, f32::INFINITY)] {
        panic_probe::assert_panics_with(domain::ANGLE_RULE, || Shape::arc(start_angle, sweep));
    }
    let bezier = CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::X,
        p2: Vec2::Y,
        p3: Vec2::ONE,
    };
    let off = Vec2::new(N, 0.0);
    for bad in [
        CubicBezier { p0: off, ..bezier },
        CubicBezier { p1: off, ..bezier },
        CubicBezier { p2: off, ..bezier },
        CubicBezier { p3: off, ..bezier },
    ] {
        panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
            check(&(Shape::cubic_bezier(bad), thin()));
        });
    }
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        check(&(Shape::line(Vec2::ZERO, Vec2::X), Stroke::new(white, -1.0)));
    });

    // Geometry is checked as offsets at every door; a varying polyline color as a color.
    let geometry: [fn(); 9] = [
        || check(&(Shape::rect().at(Rect::new(0.0, N, 8.0, 8.0)), Paint::NONE)),
        || {
            check(&(
                Shape::rect().at(Rect::new(f32::INFINITY, 0.0, 8.0, 8.0)),
                Paint::NONE,
            ));
        },
        || {
            check(&(
                Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::new(N, 0.0)),
                RgbaF32::WHITE,
            ));
        },
        || check(&(Shape::line(Vec2::ZERO, Vec2::new(0.0, N)), thin())),
        || {
            check(&(
                Shape::quadratic_bezier(QuadraticBezier {
                    p0: Vec2::ZERO,
                    p1: Vec2::X,
                    p2: Vec2::new(0.0, f32::INFINITY),
                }),
                thin(),
            ));
        },
        || check(&(Shape::polyline(&[Vec2::ZERO, Vec2::new(N, 1.0)]), thin())),
        || {
            drop(Mesh::filled_polygon(
                &[Vec2::new(N, 0.0), Vec2::X, Vec2::Y],
                RgbaF32::WHITE,
            ));
        },
        || {
            check(&(Shape::rect().at(Rect::new(0.0, 0.0, N, 8.0)), Shadow::NONE));
        },
        || {
            let mut h = Harness::mono(UVec2::ONE);
            let text = h.ui_mut().intern("t");
            check(&(
                Shape::text(text, GlyphFont::new(16.0)).at_origin(Vec2::new(N, 0.0)),
                RgbaF32::WHITE,
            ));
        },
    ];
    for build in geometry {
        panic_probe::assert_panics_with(domain::OFFSET_RULE, build);
    }
    panic_probe::assert_panics_with(domain::COLOR_RULE, || {
        let colors = [white, RgbaF32::new(N, 0.0, 0.0, 1.0)];
        check(&(
            Shape::polyline(&[Vec2::ZERO, Vec2::X]).point_colors(&colors),
            thin(),
        ));
    });
    // Each color multiplies the stroke color into one f16 lane: 255 · 256 = 65280 fits under
    // 65504, and 256 · 256 = 65536 does not, though each factor is a color.
    let bright = Stroke::new(RgbaF32::new(256.0, 1.0, 1.0, 1.0), 1.0);
    let two = [Vec2::ZERO, Vec2::X];
    check(&(
        Shape::polyline(&two).segment_colors(&[RgbaF32::new(255.0, 1.0, 1.0, 1.0)]),
        bright,
    ));
    panic_probe::assert_panics_with(COLOR_PRODUCT_RULE, || {
        let colors = [white, RgbaF32::new(256.0, 1.0, 1.0, 1.0)];
        check(&(Shape::polyline(&two).point_colors(&colors), bright));
    });
    panic_probe::assert_panics_with(COLOR_PRODUCT_RULE, || {
        check(&(
            Shape::polyline(&two).segment_colors(&[RgbaF32::new(256.0, 1.0, 1.0, 1.0)]),
            bright,
        ));
    });
    check(&(
        Shape::rect().at(Rect::new(0.0, 0.0, -4.0, 8.0)),
        Paint::NONE,
    ));

    // A tile's phase is an offset and each axis of its repeat count positive.
    let tile = |offset, scale| {
        check(&(
            Shape::rect(),
            ImagePaint::new(internals::image_handle(1)).tile(offset, scale),
        ));
    };
    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        tile(Vec2::new(f32::INFINITY, 0.0), Vec2::ONE);
    });
    for bad in [0.0, -1.0, N, f32::INFINITY] {
        panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
            tile(Vec2::ZERO, Vec2::new(1.0, bad));
        });
    }
    tile(Vec2::new(-0.5, 3.0), Vec2::new(0.25, 4.0));
}

fn thin() -> Stroke {
    Stroke::new(RgbaF32::WHITE, 1.0)
}

/// Check a shape and its paint as [`Ui::add_shape`](crate::Ui::add_shape) does.
#[track_caller]
fn check<S: DrawWith<P>, P: Clone>((shape, paint): &(S, P)) {
    let _ = internals::check(shape, paint.clone());
}
