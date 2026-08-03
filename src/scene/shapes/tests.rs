use crate::primitives::color::Color;
use crate::primitives::image::Image;
use crate::primitives::rect::Rect;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::Shapes;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use glam::Vec2;
use std::num::NonZeroU32;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Copy, Debug)]
enum ColorSource {
    Single,
    PerPoint,
    PerSegment,
}

impl ColorSource {
    fn colors<'a>(self, colors: &'a [Color]) -> PolylineColors<'a> {
        match self {
            ColorSource::Single => PolylineColors::Single(Color::WHITE),
            ColorSource::PerPoint => PolylineColors::PerPoint(colors),
            ColorSource::PerSegment => PolylineColors::PerSegment(colors),
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

#[test]
fn polyline_color_cardinality_is_enforced_before_noop_lowering() {
    let points = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    let colors = [Color::WHITE; 3];

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
                let store = RecordStore::default();
                let shape = Shape::polyline(
                    &points[..points_len],
                    source.colors(&colors[..colors_len]),
                    1.0,
                );
                let result = catch_unwind(AssertUnwindSafe(|| shapes.add(shape.into(), &store)));
                let accepted = source.accepts(points_len, colors_len);

                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );

                if !accepted {
                    assert!(shapes.records.is_empty());
                    assert!(shapes.hashes.is_empty());
                    let payloads = store.payloads.borrow();
                    assert!(payloads.polyline_points.is_empty());
                    assert!(payloads.polyline_colors.is_empty());
                    continue;
                }

                let stored = points_len == 2;
                assert_eq!(result.unwrap(), stored.then_some(0));
                assert_eq!(shapes.records.len(), usize::from(stored));
                assert_eq!(shapes.hashes.len(), usize::from(stored));
                let payloads = store.payloads.borrow();
                assert_eq!(
                    payloads.polyline_points.len(),
                    points_len * usize::from(stored)
                );
                assert_eq!(
                    payloads.polyline_colors.len(),
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
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let registry = ImageRegistry::new(
        TextureIdSource::default(),
        Some(NonZeroU32::new(WIDTH).unwrap()),
    );
    let handle = registry
        .register(Image::from_rgba8(WIDTH, 1, vec![0; WIDTH as usize * 4]))
        .unwrap();
    let mut shapes = Shapes::default();
    let store = RecordStore::default();

    assert_eq!(shapes.add(Shape::image(handle).into(), &store), Some(0));
    let ShapeRecord::Image {
        source: ImageSource::Texture { size, .. },
        ..
    } = shapes.records[0]
    else {
        panic!("image lowered to another record variant or source");
    };
    assert_eq!(size, glam::UVec2::new(WIDTH, 1));
}

/// **The NaN contract**, exercised through `Shapes::add` for every
/// shape kind: a NaN anywhere in a shape's inputs means the shape is
/// **never recorded**. Its clean twin must record, so a gate that
/// rejected everything would fail this too.
///
/// The bulk cases are the point of the design: a NaN polyline point,
/// mesh vertex, or curve control point is caught via the `bbox` it folds
/// into, not by rescanning the data — which is what keeps the check
/// `O(1)` and affordable in release rather than debug-only.
///
/// Two doors lead to "not recorded", and which one a case takes is not
/// pinned here because it is not part of the contract: a NaN that also
/// reads as invisible (a NaN origin makes `is_paint_empty` true) exits
/// through the ordinary no-op gate, quietly; one that would otherwise
/// have painted reaches `Shapes::add`'s NaN gate and additionally
/// asserts in debug. Both drop the shape, which is what callers can
/// rely on.
#[test]
fn the_nan_gate_drops_every_shape_kind() {
    use crate::primitives::mesh::Mesh;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::shape::Shape;
    use glam::Vec2;

    const N: f32 = f32::NAN;
    let nan_pt = Vec2::new(1.0, N);
    let ok_rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let white = Color::WHITE;
    let mesh = |pos| {
        let mut m = Mesh::new();
        m.vertex(pos, white);
        m.vertex(Vec2::new(4.0, 0.0), white);
        m.vertex(Vec2::new(0.0, 4.0), white);
        m.triangle(0, 1, 2);
        m
    };
    let (mesh_nan, mesh_ok) = (mesh(nan_pt), mesh(Vec2::ZERO));
    let pts_nan = [Vec2::ZERO, nan_pt, Vec2::new(4.0, 4.0)];
    let pts_ok = [Vec2::ZERO, Vec2::new(2.0, 2.0), Vec2::new(4.0, 4.0)];

    // (label, tainted, clean)
    let cases: Vec<(&str, Shape<'_>, Shape<'_>)> = vec![
        (
            "rect_local_rect",
            Shape::rect(Rect::new(0.0, N, 8.0, 8.0)).fill(white).into(),
            Shape::rect(ok_rect).fill(white).into(),
        ),
        (
            "rect_corners",
            Shape::rect(ok_rect).fill(white).corners(N).into(),
            Shape::rect(ok_rect).fill(white).corners(2.0).into(),
        ),
        (
            "rect_stroke_colour",
            Shape::rect(ok_rect)
                .fill(white)
                .stroke(Stroke::solid(Color::rgba(0.0, N, 0.0, 1.0), 2.0))
                .into(),
            Shape::rect(ok_rect)
                .fill(white)
                .stroke(Stroke::solid(Color::BLACK, 2.0))
                .into(),
        ),
        (
            "triangle_corner",
            Shape::triangle(Vec2::ZERO, Vec2::new(4.0, 0.0), nan_pt)
                .fill(white)
                .into(),
            Shape::triangle(Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0))
                .fill(white)
                .into(),
        ),
        (
            "curve_control_point",
            Shape::line(Vec2::ZERO, nan_pt, 2.0).brush(white).into(),
            Shape::line(Vec2::ZERO, Vec2::new(4.0, 4.0), 2.0)
                .brush(white)
                .into(),
        ),
        (
            "arc_centre",
            Shape::arc(nan_pt, 4.0, 0.0, 1.0, 2.0).brush(white).into(),
            Shape::arc(Vec2::ZERO, 4.0, 0.0, 1.0, 2.0)
                .brush(white)
                .into(),
        ),
        (
            "polyline_point",
            Shape::polyline(&pts_nan, PolylineColors::Single(white), 2.0).into(),
            Shape::polyline(&pts_ok, PolylineColors::Single(white), 2.0).into(),
        ),
        (
            "mesh_vertex",
            Shape::mesh(&mesh_nan).into(),
            Shape::mesh(&mesh_ok).into(),
        ),
        (
            "shadow_blur",
            Shape::shadow(Shadow {
                color: white,
                blur: N,
                ..Shadow::default()
            })
            .at(ok_rect)
            .into(),
            Shape::shadow(Shadow {
                color: white,
                blur: 4.0,
                ..Shadow::default()
            })
            .at(ok_rect)
            .into(),
        ),
    ];

    for (label, tainted, clean) in cases {
        let mut shapes = Shapes::default();
        let store = RecordStore::default();
        let got = catch_unwind(AssertUnwindSafe(|| shapes.add(tainted.clone(), &store)));
        assert_eq!(
            got.unwrap_or(None),
            None,
            "case {label}: a NaN shape must never be recorded",
        );
        assert!(
            shapes.records.is_empty(),
            "case {label}: nothing may reach the record buffer",
        );
        // A rejected shape must leave no trace in the payload arena
        // either. Polyline is the case with teeth: it is the one shape
        // that reaches lowering with its NaN intact, so its bail has to
        // come before it stages anything.
        let payloads = store.payloads.borrow();
        assert!(
            payloads.polyline_points.is_empty()
                && payloads.polyline_colors.is_empty()
                && payloads.meshes.vertices.is_empty()
                && payloads.meshes.indices.is_empty(),
            "case {label}: a rejected shape left bytes in the arena",
        );

        let mut shapes = Shapes::default();
        let store = RecordStore::default();
        assert_eq!(
            shapes.add(clean, &store),
            Some(0),
            "case {label}: the clean twin must record — otherwise the \
             tainted arm proves nothing",
        );
    }
}
