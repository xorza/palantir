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

/// The colour-cardinality contract is checked where it is consumed —
/// `lower::polyline` — not by the no-op query that used to open with it. So a
/// polyline that never lowers (fewer than two points) is dropped in silence
/// whatever its colour slice says, and one that does lower is checked.
#[test]
fn polyline_color_cardinality_is_enforced_at_lowering() {
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
                let mut store = RecordStore::default();
                let shape = Shape::polyline(
                    &points[..points_len],
                    source.colors(&colors[..colors_len]),
                    1.0,
                );
                let result = catch_unwind(AssertUnwindSafe(|| shapes.add(shape, &mut store)));
                // What the no-op gate drops never reaches lowering, and so is
                // never checked: fewer than two points, or a per-vertex colour
                // slice with nothing visible in it — which an *empty* slice is,
                // vacuously. What does lower is checked, and a mismatch panics
                // in a debug build (the check is `debug_assert`, off the
                // release paint path).
                let colors_invisible =
                    matches!(source, ColorSource::PerPoint | ColorSource::PerSegment)
                        && colors_len == 0;
                let lowers = points_len >= 2 && !colors_invisible;
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
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let registry = ImageRegistry::new(TextureIdSource::default());
    let handle = registry.register(Image::from_rgba8(WIDTH, 1, vec![0; WIDTH as usize * 4]));
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
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::color::ColorU8;
    use crate::primitives::mesh::Mesh;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::shape::{Lower, Shape};
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

    // A generic helper, one call per case: without the erased `Shape`
    // enum the kinds no longer share a type, so they cannot sit in one
    // table. Each call monomorphizes, which is also what the production
    // path now does.
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
        // A rejected shape must leave no trace in the payload arena
        // either, which is what places the screen before lowering rather
        // than on the record it produces: a mesh copies its vertices, a
        // gradient fill interns a row, and a text run copies its bytes,
        // all before a record exists to be judged.
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

    let tri = |c, r: f32| {
        Shape::triangle(Vec2::ZERO, Vec2::new(4.0, 0.0), c)
            .fill(white)
            .radius(r)
    };
    gate(
        "rect_local_rect",
        Shape::rect(Rect::new(0.0, N, 8.0, 8.0)).fill(white),
        Shape::rect(ok_rect).fill(white),
    );
    gate(
        "rect_corners",
        Shape::rect(ok_rect).fill(white).corners(N),
        Shape::rect(ok_rect).fill(white).corners(2.0),
    );
    gate(
        "rect_stroke_colour",
        Shape::rect(ok_rect)
            .fill(white)
            .stroke(Stroke::solid(Color::rgba(0.0, N, 0.0, 1.0), 2.0)),
        Shape::rect(ok_rect)
            .fill(white)
            .stroke(Stroke::solid(Color::BLACK, 2.0)),
    );
    gate(
        "triangle_corner",
        tri(nan_pt, 0.0),
        tri(Vec2::new(0.0, 4.0), 0.0),
    );
    // `radius` reaches lowering only through `radius.max(0.0)`, which
    // launders NaN to `0.0`, so the bbox it would have shown up in comes
    // out finite and only the authored screen can catch it.
    gate(
        "triangle_radius",
        tri(Vec2::new(0.0, 4.0), N),
        tri(Vec2::new(0.0, 4.0), 1.0),
    );
    gate(
        "curve_control_point",
        Shape::line(Vec2::ZERO, nan_pt, 2.0).brush(white),
        Shape::line(Vec2::ZERO, Vec2::new(4.0, 4.0), 2.0).brush(white),
    );
    gate(
        "arc_centre",
        Shape::arc(nan_pt, 4.0, 0.0, 1.0, 2.0).brush(white),
        Shape::arc(Vec2::ZERO, 4.0, 0.0, 1.0, 2.0).brush(white),
    );
    gate(
        "polyline_point",
        Shape::polyline(&pts_nan, PolylineColors::Single(white), 2.0),
        Shape::polyline(&pts_ok, PolylineColors::Single(white), 2.0),
    );
    gate("mesh_vertex", Shape::mesh(&mesh_nan), Shape::mesh(&mesh_ok));
    gate(
        "mesh_local_rect",
        Shape::mesh(&mesh_ok).at(Rect::new(0.0, N, 8.0, 8.0)),
        Shape::mesh(&mesh_ok).at(ok_rect),
    );
    // A gradient's geometry is the one authoring input that does not
    // survive lowering: it interns behind a `GradientId`, so a record
    // gate could not see it and a shape rejected afterwards would leave
    // the pool row behind. The arena check above is what pins that.
    let gradient = |angle| {
        Shape::rect(ok_rect).fill(LinearGradient::two_stop(
            angle,
            ColorU8::hex(0x1a1a2e),
            ColorU8::hex(0x4c5cdb),
        ))
    };
    gate("rect_gradient_geometry", gradient(N), gradient(0.25));
    let shadow = |blur| {
        Shape::shadow(Shadow {
            color: white,
            blur,
            ..Shadow::default()
        })
        .at(ok_rect)
    };
    gate("shadow_blur", shadow(N), shadow(4.0));
}
