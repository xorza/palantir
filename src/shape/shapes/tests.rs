use crate::image_registry::ImageRegistry;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::polyline::PolylineShape;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::shapes::Shapes;
use glam::UVec2;
use glam::Vec2;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::slice;

#[derive(Clone, Copy, Debug)]
enum ColorSource {
    Single,
    PerPoint,
    PerSegment,
}

impl ColorSource {
    fn apply<'a>(self, shape: PolylineShape<'a>, colors: &'a [RgbaF32]) -> PolylineShape<'a> {
        match self {
            Self::Single => shape,
            Self::PerPoint => shape.per_point(colors),
            Self::PerSegment => shape.per_segment(colors),
        }
    }

    fn accepts(self, points_len: usize, colors_len: usize) -> bool {
        match self {
            Self::Single => true,
            Self::PerPoint => colors_len == points_len,
            Self::PerSegment => colors_len == points_len.saturating_sub(1),
        }
    }

    fn stored_colors_len(self, points_len: usize) -> u32 {
        match self {
            Self::Single => 1,
            Self::PerPoint => points_len as u32,
            Self::PerSegment => points_len.saturating_sub(1) as u32,
        }
    }
}

/// The color-cardinality contract is checked by the color setter, whatever the point count; an
/// accepted polyline under two points is dropped, and one of two or more lowers its colors.
#[test]
fn polyline_color_cardinality_is_enforced_at_the_setter() {
    let points = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let colors = [
        RgbaF32::new(1.0, 1.0, 1.0, 1.0),
        RgbaF32::new(0.5, 1.0, 0.5, 1.0),
        RgbaF32::new(0.0, 0.5, 1.0, 0.5),
    ];
    // `tint` × each color; every product is a power-of-two fraction, exact in f16.
    let tinted = [
        RgbaF32::new(0.5, 0.25, 1.0, 0.5),
        RgbaF32::new(0.25, 0.25, 0.5, 0.5),
        RgbaF32::new(0.0, 0.125, 1.0, 0.25),
    ];

    for points_len in 0..=2 {
        for source in [
            ColorSource::Single,
            ColorSource::PerPoint,
            ColorSource::PerSegment,
        ] {
            let color_lengths: &[usize] = match source {
                ColorSource::Single => &[0],
                ColorSource::PerPoint | ColorSource::PerSegment => &[0, 1, 2, 3],
            };

            for &colors_len in color_lengths {
                let shape = catch_unwind(AssertUnwindSafe(|| {
                    source.apply(
                        Shape::polyline(&points[..points_len], Stroke::new(tint, 1.0)),
                        &colors[..colors_len],
                    )
                }));
                let accepted = source.accepts(points_len, colors_len);
                assert_eq!(
                    shape.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );
                let Ok(shape) = shape else {
                    continue;
                };

                let mut shapes = Shapes::default();
                let mut store = RecordStore::default();
                let stored = points_len >= 2;
                assert_eq!(shapes.add(shape, &mut store), stored.then_some(0));
                assert_eq!(shapes.records.len(), usize::from(stored));
                assert_eq!(shapes.hashes.len(), usize::from(stored));
                assert_eq!(
                    store.polyline_points.len(),
                    points_len * usize::from(stored)
                );
                assert_eq!(
                    store.polyline_colors.len(),
                    source.stored_colors_len(points_len) as usize * usize::from(stored),
                );

                if stored {
                    let ShapeRecord::Polyline {
                        points: point_span,
                        colors: color_span,
                        ..
                    } = &shapes.records[0]
                    else {
                        panic!("accepted polyline lowered to another record variant");
                    };
                    assert_eq!(point_span.len, points_len as u32);
                    assert_eq!(color_span.len, source.stored_colors_len(points_len));
                    let expected = match source {
                        ColorSource::Single => slice::from_ref(&tint),
                        ColorSource::PerPoint | ColorSource::PerSegment => &tinted[..colors_len],
                    };
                    let expected: Vec<RgbaF16> = expected.iter().map(|&c| c.to_f16()).collect();
                    assert_eq!(
                        &store.polyline_colors[color_span.range()],
                        expected.as_slice(),
                        "{source:?}: staged colors are the stroke color times each color",
                    );
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let mut shapes = Shapes::default();
    let mut store = RecordStore::default();

    assert_eq!(shapes.add(Shape::image(handle(WIDTH)), &mut store), Some(0));
    let ShapeRecord::Image {
        source: ImageSource::Texture { size, .. },
        ..
    } = shapes.records[0]
    else {
        panic!("image lowered to another record variant or source");
    };
    assert_eq!(size, UVec2::new(WIDTH, 1));
}

/// Inputs a builder checks never reach the record gate: each panics with its
/// kind's rule on entry. Gradient geometry most of all, as it interns behind a
/// `GradientId` the gate cannot see.
#[test]
fn builders_refuse_what_they_check() {
    use crate::internals::panic_probe;
    use crate::internals::ui_harness::UiHarness;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::cubic_bezier::CubicBezier;
    use crate::primitives::geometry::mesh::Mesh;
    use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
    use crate::primitives::math::domain;
    use crate::primitives::packed::serde::LaneCodec;
    use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::paint::lowered_shadow::LoweredShadow;
    use crate::shape::polyline::TINT_RULE;
    use crate::text::glyph_font::GlyphFont;

    const N: f32 = f32::NAN;
    let ok_rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let white = RgbaF32::WHITE;
    for radius in [N, -1.0, 1.0e5] {
        panic_probe::assert_panics_with(<Corners as LaneCodec>::LANE_RULE, || {
            Shape::rect(ok_rect).fill(white).corners(radius)
        });
    }
    panic_probe::assert_panics_with("a color must have finite channels of at most 65504", || {
        Shape::rect(ok_rect).border(Stroke::new(RgbaF32::srgba(0.0, N, 0.0, 1.0), 2.0))
    });
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y).radius(N)
    });
    panic_probe::assert_panics_with(domain::ANGLE_RULE, || {
        Shape::rect(ok_rect).fill(LinearGradient::two_stop(N, white, RgbaF32::BLACK))
    });
    // 65520 is the least magnitude that packs to an infinite f16 lane; 65504 is the greatest
    // finite one. A gradient's center and radius and every shadow scalar pack into such lanes.
    let wide = 65520.0;
    let half = Vec2::splat(0.5);
    let radial = |center, radius| {
        RadialGradient::builder(center, radius)
            .stop(0.0, white)
            .stop(1.0, RgbaF32::BLACK)
            .build()
    };
    panic_probe::assert_panics_with(domain::LANE_OFFSET_RULE, || {
        Shape::rect(ok_rect).fill(radial(Vec2::new(wide, 0.5), half))
    });
    panic_probe::assert_panics_with(domain::GAP_RULE, || {
        Shape::rect(ok_rect).fill(radial(half, Vec2::new(0.5, wide)))
    });
    panic_probe::assert_panics_with(domain::LANE_OFFSET_RULE, || {
        Shape::rect(ok_rect).fill(
            ConicGradient::builder(Vec2::new(0.5, -wide), 0.0)
                .stop(0.0, white)
                .stop(1.0, RgbaF32::BLACK)
                .build(),
        )
    });
    let _ = Shape::rect(ok_rect).fill(radial(Vec2::splat(-65504.0), Vec2::splat(65504.0)));
    for (rule, shadow) in [
        (
            domain::GAP_RULE,
            Shadow {
                blur: N,
                ..Shadow::default()
            },
        ),
        (
            domain::GAP_RULE,
            Shadow {
                blur: wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_OFFSET_RULE,
            Shadow {
                spread: wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_OFFSET_RULE,
            Shadow {
                spread: -wide,
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_OFFSET_RULE,
            Shadow {
                offset: Vec2::new(wide, 0.0),
                ..Shadow::default()
            },
        ),
        (
            domain::LANE_OFFSET_RULE,
            Shadow {
                offset: Vec2::new(0.0, -wide),
                ..Shadow::default()
            },
        ),
    ] {
        panic_probe::assert_panics_with(rule, || {
            Shape::shadow(Shadow {
                color: white,
                ..shadow
            })
        });
    }
    let widest = Shadow {
        color: white,
        offset: Vec2::splat(-65504.0),
        blur: 65504.0,
        spread: 65504.0,
        inset: false,
    };
    let lowered = LoweredShadow::from(widest).geometry();
    assert_eq!(
        (lowered.offset, lowered.blur, lowered.spread),
        (widest.offset, widest.blur, widest.spread),
        "the widest shadow packs exactly"
    );
    let _ = Shape::shadow(widest);
    let arc = CircularArc {
        center: Vec2::ZERO,
        radius: 4.0,
        start_angle: 0.0,
        sweep: 1.0,
    };
    for (rule, bad) in [
        (
            domain::OFFSET_RULE,
            CircularArc {
                center: Vec2::new(0.0, N),
                ..arc
            },
        ),
        (
            domain::LENGTH_RULE,
            CircularArc {
                radius: -1.0,
                ..arc
            },
        ),
        (
            domain::ANGLE_RULE,
            CircularArc {
                start_angle: N,
                ..arc
            },
        ),
        (
            domain::ANGLE_RULE,
            CircularArc {
                sweep: f32::INFINITY,
                ..arc
            },
        ),
    ] {
        panic_probe::assert_panics_with(rule, || Shape::arc(bad, thin()));
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
        panic_probe::assert_panics_with(domain::OFFSET_RULE, || Shape::cubic_bezier(bad, thin()));
    }
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        Shape::line(Vec2::ZERO, Vec2::X, Stroke::new(white, -1.0))
    });

    // Geometry is checked as offsets at every door; a varying polyline color as a color.
    let geometry: [fn(); 10] = [
        || drop(Shape::rect(Rect::new(0.0, N, 8.0, 8.0))),
        || {
            drop(Shape::windowed_rect(Rect::new(
                f32::INFINITY,
                0.0,
                8.0,
                8.0,
            )));
        },
        || drop(Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::new(N, 0.0))),
        || drop(Shape::line(Vec2::ZERO, Vec2::new(0.0, N), thin())),
        || {
            drop(Shape::quadratic_bezier(
                QuadraticBezier {
                    p0: Vec2::ZERO,
                    p1: Vec2::X,
                    p2: Vec2::new(0.0, f32::INFINITY),
                },
                thin(),
            ));
        },
        || drop(Shape::circle(Vec2::new(N, 0.0), 4.0, thin())),
        || drop(Shape::polyline(&[Vec2::ZERO, Vec2::new(N, 1.0)], thin())),
        || {
            drop(Mesh::filled_triangle(
                [Vec2::new(N, 0.0), Vec2::X, Vec2::Y],
                RgbaF32::WHITE,
            ));
        },
        || drop(Shape::shadow(Shadow::NONE).at(Rect::new(0.0, 0.0, N, 8.0))),
        || {
            let mut h = UiHarness::arena();
            let text = h.ui().intern("t");
            drop(Shape::text(text, GlyphFont::new(16.0)).at_origin(Vec2::new(N, 0.0)));
        },
    ];
    for build in geometry {
        panic_probe::assert_panics_with(domain::OFFSET_RULE, build);
    }
    panic_probe::assert_panics_with(domain::COLOR_RULE, || {
        let colors = [white, RgbaF32::new(N, 0.0, 0.0, 1.0)];
        drop(Shape::polyline(&[Vec2::ZERO, Vec2::X], thin()).per_point(&colors));
    });
    // Each color multiplies the stroke color into one f16 lane: 255 · 256 = 65280 fits under
    // 65504, and 256 · 256 = 65536 does not, though each factor is a color.
    let bright = Stroke::new(RgbaF32::new(256.0, 1.0, 1.0, 1.0), 1.0);
    let two = [Vec2::ZERO, Vec2::X];
    let _ = Shape::polyline(&two, bright).per_segment(&[RgbaF32::new(255.0, 1.0, 1.0, 1.0)]);
    panic_probe::assert_panics_with(TINT_RULE, || {
        let colors = [white, RgbaF32::new(256.0, 1.0, 1.0, 1.0)];
        drop(Shape::polyline(&two, bright).per_point(&colors));
    });
    panic_probe::assert_panics_with(TINT_RULE, || {
        drop(Shape::polyline(&two, bright).per_segment(&[RgbaF32::new(256.0, 1.0, 1.0, 1.0)]));
    });
    let _ = Shape::rect(Rect::new(0.0, 0.0, -4.0, 8.0));

    // A tile's phase is an offset and each axis of its repeat count positive.
    let tile = |offset, scale| drop(Shape::image(handle(1)).fit(ImageFit::Tile { offset, scale }));
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

/// A handle to a blank `width` × 1 image.
fn handle(width: u32) -> ImageHandle {
    ImageHandle::new(
        TextureId(1),
        &Image::from_srgba8(UVec2::new(width, 1), vec![0; width as usize * 4]).unwrap(),
        ImageRegistry::default(),
    )
}

fn thin() -> Stroke {
    Stroke::new(RgbaF32::WHITE, 1.0)
}

/// Debug-only: a shadow that skipped `Shadow::validate` fails where it lowers, at 65520, the
/// least magnitude that packs to an infinite f16 lane.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a shadow scalar packs past the largest f16")]
fn a_shadow_past_the_largest_f16_fails_where_it_lowers() {
    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::paint::lowered_shadow::LoweredShadow;

    let _ = LoweredShadow::from(Shadow {
        blur: 65520.0,
        ..Shadow::default()
    });
}
