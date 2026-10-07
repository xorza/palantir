use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::scene::record_store::RecordStore;
use crate::shape::Shape;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::polyline::PolylineShape;
use crate::shape::record::ShapeRecord;
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
            ColorSource::Single => shape,
            ColorSource::PerPoint => shape.per_point(colors),
            ColorSource::PerSegment => shape.per_segment(colors),
        }
    }

    fn accepts(self, points_len: usize, colors_len: usize) -> bool {
        match self {
            ColorSource::Single => true,
            ColorSource::PerPoint => colors_len == points_len,
            ColorSource::PerSegment => colors_len == points_len.saturating_sub(1),
        }
    }

    fn stored_colors_len(self, points_len: usize) -> u32 {
        match self {
            ColorSource::Single => 1,
            ColorSource::PerPoint => points_len as u32,
            ColorSource::PerSegment => points_len.saturating_sub(1) as u32,
        }
    }
}

/// The colour-cardinality contract is checked at `lower::polyline`: a polyline
/// under two points is dropped whatever its colour slice, and a wrong-length
/// slice is never a no-op, so it lowers and is checked.
#[test]
fn polyline_color_cardinality_is_enforced_at_lowering() {
    let points = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let colors = [
        RgbaF32::new(1.0, 1.0, 1.0, 1.0),
        RgbaF32::new(0.5, 1.0, 0.5, 1.0),
        RgbaF32::new(0.0, 0.5, 1.0, 0.5),
    ];
    // `tint` × each colour; every product is a power-of-two fraction, exact in f16.
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
                let mut shapes = Shapes::default();
                let mut store = RecordStore::default();
                let shape = source.apply(
                    Shape::polyline(&points[..points_len], Stroke::new(tint, 1.0)),
                    &colors[..colors_len],
                );
                let result = catch_unwind(AssertUnwindSafe(|| shapes.add(shape, &mut store)));
                // A wrong-length colour slice (empty included) is never a no-op, so the cardinality assert panics.
                let lowers = points_len >= 2;
                let accepted = !lowers || source.accepts(points_len, colors_len);

                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );

                if !accepted {
                    assert!(shapes.records.is_empty());
                    assert!(shapes.hashes.is_empty());
                    assert!(store.polyline_points.is_empty());
                    assert!(store.polyline_colors.is_empty());
                    continue;
                }

                let stored = lowers;
                assert_eq!(result.unwrap(), stored.then_some(0));
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
                    let expected: Vec<RgbaF16> = expected.iter().map(|&c| c.into()).collect();
                    assert_eq!(
                        &store.polyline_colors[color_span.range()],
                        expected.as_slice(),
                        "{source:?}: staged colours are the stroke colour times each colour",
                    );
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let handle = ImageHandle::new(
        TextureId(1),
        &Image::from_srgba8(UVec2::new(WIDTH, 1), vec![0; WIDTH as usize * 4]).unwrap(),
        ImageRegistry::default(),
    );
    let mut shapes = Shapes::default();
    let mut store = RecordStore::default();

    assert_eq!(shapes.add(Shape::image(handle), &mut store), Some(0));
    let ShapeRecord::Image {
        source: ImageSource::Texture { size, .. },
        ..
    } = shapes.records[0]
    else {
        panic!("image lowered to another record variant or source");
    };
    assert_eq!(size, UVec2::new(WIDTH, 1));
}

/// **The NaN contract**, through `Shapes::add` for every kind: a shape with a
/// NaN input is never recorded, and its clean twin must record. Bulk NaNs are
/// caught via the `bbox` they fold into, keeping the check `O(1)`. Which door
/// drops it (the no-op gate or `Shapes::add`'s NaN gate) is not pinned.
#[test]
fn the_nan_gate_drops_every_shape_kind() {
    // One generic helper per case: without the erased `Shape` enum the kinds share no type.
    #[track_caller]
    fn gate<T: Lower, C: Lower>(label: &str, tainted: T, clean: C) {
        let mut shapes = Shapes::default();
        let mut store = RecordStore::default();
        let got = catch_unwind(AssertUnwindSafe(|| shapes.add(tainted, &mut store)));
        assert_eq!(
            got.unwrap_or(None),
            None,
            "case {label}: a NaN shape must never be recorded",
        );
        assert!(
            shapes.records.is_empty(),
            "case {label}: nothing may reach the record buffer",
        );
        // A rejected shape must leave nothing in the payload arena, so the
        // screen runs before lowering copies vertices, interns rows or copies text.
        {
            assert!(
                store.polyline_points.is_empty()
                    && store.polyline_colors.is_empty()
                    && store.meshes.vertices.is_empty()
                    && store.meshes.indices.is_empty()
                    && store.gradients.records.is_empty(),
                "case {label}: a rejected shape left bytes in the arena",
            );
        }

        let mut shapes = Shapes::default();
        let mut store = RecordStore::default();
        assert_eq!(
            shapes.add(clean, &mut store),
            Some(0),
            "case {label}: the clean twin must record — otherwise the \
             tainted arm proves nothing",
        );
    }

    use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::Lower;
    use crate::shape::curve::{CurveGeometry, CurveShape};
    use crate::shape::mesh::MeshShape;
    use crate::shape::polyline::PolylineColors;
    use crate::shape::rect::{RectKind, RectShape};
    use crate::shape::triangle::TriangleShape;

    // Public builders refuse non-finite input; the gate backstops a path that skipped one.
    const N: f32 = f32::NAN;
    let nan_pt = Vec2::new(1.0, N);
    let ok_rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let white = RgbaF32::WHITE;
    let stroke = Stroke::new(white, 2.0);
    let mesh = |pos| {
        let mut m = Mesh::new();
        m.vertices.push(MeshVertex::new(pos, white));
        m.vertices.push(MeshVertex::new(Vec2::new(4.0, 0.0), white));
        m.vertices.push(MeshVertex::new(Vec2::new(0.0, 4.0), white));
        m.triangle(0, 1, 2);
        m
    };
    let (mesh_nan, mesh_ok) = (mesh(nan_pt), mesh(Vec2::ZERO));
    let pts_nan = [Vec2::ZERO, nan_pt, Vec2::new(4.0, 4.0)];
    let pts_ok = [Vec2::ZERO, Vec2::new(2.0, 2.0), Vec2::new(4.0, 4.0)];
    let tri = |c| TriangleShape::new(Vec2::ZERO, Vec2::new(4.0, 0.0), c).fill(white);
    let rect = |r| RectShape::new(RectKind::Rounded, Some(r)).fill(white);
    let line = |b| CurveShape::new(CurveGeometry::Line { a: Vec2::ZERO, b }, stroke);
    let arc = |center| {
        CurveShape::new(
            CurveGeometry::Arc {
                center,
                radius: 4.0,
                start_angle: 0.0,
                sweep: 1.0,
            },
            stroke,
        )
    };
    gate(
        "rect_local_rect",
        rect(Rect::new(0.0, N, 8.0, 8.0)),
        rect(ok_rect),
    );
    gate("triangle_corner", tri(nan_pt), tri(Vec2::new(0.0, 4.0)));
    gate(
        "curve_control_point",
        line(nan_pt),
        line(Vec2::new(4.0, 4.0)),
    );
    gate("arc_centre", arc(nan_pt), arc(Vec2::ZERO));
    gate(
        "polyline_point",
        PolylineShape::new(&pts_nan, stroke),
        PolylineShape::new(&pts_ok, stroke),
    );
    let nan_red = RgbaF32::new(N, 0.0, 0.0, 1.0);
    let colors_nan = [white, nan_red, white];
    let colors_ok = [white; 3];
    let colored = |colors| PolylineShape {
        colors,
        ..PolylineShape::new(&pts_ok, stroke)
    };
    gate(
        "polyline_point_colour",
        colored(PolylineColors::PerPoint(&colors_nan)),
        colored(PolylineColors::PerPoint(&colors_ok)),
    );
    gate(
        "polyline_segment_colour",
        colored(PolylineColors::PerSegment(&colors_nan[1..])),
        colored(PolylineColors::PerSegment(&colors_ok[1..])),
    );
    gate(
        "mesh_vertex",
        MeshShape::new(&mesh_nan),
        MeshShape::new(&mesh_ok),
    );
    gate(
        "mesh_local_rect",
        MeshShape {
            local_rect: Some(Rect::new(0.0, N, 8.0, 8.0)),
            ..MeshShape::new(&mesh_ok)
        },
        MeshShape::new(&mesh_ok).at(ok_rect),
    );
}

/// Inputs a builder checks never reach the record gate: each panics with its
/// kind's rule on entry. Gradient geometry most of all, as it interns behind a
/// `GradientId` the gate cannot see.
#[test]
fn builders_refuse_what_they_check() {
    use crate::internals::harness::UiHarness;
    use crate::internals::panic_probe;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::mesh::Mesh;
    use crate::primitives::math::domain;
    use crate::primitives::packed::serde::LaneCodec;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::shadow::Shadow;
    use crate::text::glyph_font::GlyphFont;

    const N: f32 = f32::NAN;
    let ok_rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let white = RgbaF32::WHITE;
    for radius in [N, -1.0, 1.0e5] {
        panic_probe::assert_panics_with(<Corners as LaneCodec>::LANE_RULE, || {
            Shape::rect(ok_rect).fill(white).corners(radius)
        });
    }
    panic_probe::assert_panics_with("a color must have finite channels", || {
        Shape::rect(ok_rect).border(Stroke::new(RgbaF32::srgba(0.0, N, 0.0, 1.0), 2.0))
    });
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y).radius(N)
    });
    panic_probe::assert_panics_with(domain::ANGLE_RULE, || {
        Shape::rect(ok_rect).fill(LinearGradient::two_stop(N, white, RgbaF32::BLACK))
    });
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        Shape::shadow(Shadow {
            color: white,
            blur: N,
            ..Shadow::default()
        })
    });
    panic_probe::assert_panics_with(domain::ANGLE_RULE, || {
        Shape::arc(Vec2::ZERO, 4.0, 0.0, f32::INFINITY, Stroke::new(white, 1.0))
    });
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        Shape::line(Vec2::ZERO, Vec2::X, Stroke::new(white, -1.0))
    });

    // Geometry is checked as offsets at every door; a varying polyline colour as a colour.
    let geometry: [fn(); 11] = [
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
            drop(Shape::cubic_bezier(
                Vec2::ZERO,
                Vec2::new(N, 0.0),
                Vec2::X,
                Vec2::Y,
                thin(),
            ));
        },
        || {
            drop(Shape::quadratic_bezier(
                Vec2::ZERO,
                Vec2::X,
                Vec2::new(0.0, f32::INFINITY),
                thin(),
            ));
        },
        || drop(Shape::circle(Vec2::new(N, 0.0), 4.0, thin())),
        || drop(Shape::polyline(&[Vec2::ZERO, Vec2::new(N, 1.0)], thin())),
        || {
            drop(Mesh::filled_triangle(
                Vec2::new(N, 0.0),
                Vec2::X,
                Vec2::Y,
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
    let _ = Shape::rect(Rect::new(0.0, 0.0, -4.0, 8.0));
}

fn thin() -> Stroke {
    Stroke::new(RgbaF32::WHITE, 1.0)
}
