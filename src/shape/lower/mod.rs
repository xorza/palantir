//! Authoring → storage lowering: [`Shape`] inputs and [`Background`] chrome become the [`ShapeRecord`] / [`ChromeRow`] forms the tree stores; bulk payload appends to the window's [`RecordStore`].
//!
//! What lives here touches the store: a shape that must stage something (gradient stops, polyline points, mesh vertices) lowers here, keeping the `RecordStore` borrow off the authoring side; one that merely repacks its fields lowers in its own `Lower` impl.
//!
//! Entry points: [`Shapes::add`](crate::shape::shapes::Shapes::add) for shapes; `Tree::open_node` calls [`background`].
//!
//! [`Shape`]: crate::shape::Shape

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::arc;
use crate::primitives::geometry::bezier;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::shape::curve::{CurveGeometry, CurveStyle};
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::CurveRamp;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::polyline::PolylineColors;
use crate::shape::record::{ColorMode, ShapeRecord};
use crate::shape::rect::RectKind;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash;
use std::hash::{Hash as _, Hasher as _};
use std::slice;

/// Stable content hash for a gradient kind or curve ramp: discriminant byte then the value's `Hash`, captured at lowering so downstream cache keys stay context-free.
#[inline]
fn grad_hash<G: hash::Hash>(tag: u8, g: &G) -> u64 {
    let mut h = Hasher::new();
    h.write_u8(tag);
    g.hash(&mut h);
    h.finish()
}

fn gradient_brush<G: GradientGeometry>(
    store: &mut RecordStore,
    tag: u8,
    kind: FillKind,
    gradient: &Gradient<G>,
) -> ShapeBrush {
    let hash = grad_hash(tag, gradient);
    let id = store.intern_gradient(
        hash,
        RecordedGradient {
            axis: gradient.axis(),
            kind,
            ramp: gradient.ramp,
        },
    );
    ShapeBrush::Gradient { id, hash }
}

/// Lower a `Brush`: `Solid` stays inline; gradients go into the store and return a `ShapeBrush::Gradient` with index and content hash.
pub(crate) fn brush(store: &mut RecordStore, b: &Brush) -> ShapeBrush {
    // A gradient's geometry vanishes behind a `GradientId`, so the no-op decision is made before interning, by both callers.
    debug_assert!(
        !b.has_nan(),
        "NaN gradient geometry reached lowering: {b:?}"
    );
    match b {
        Brush::Solid(color) => ShapeBrush::Solid((*color).into()),
        Brush::Linear(g) => gradient_brush(store, 0, FillKind::linear(g.spread), g),
        Brush::Radial(g) => gradient_brush(store, 1, FillKind::radial(g.spread), g),
        Brush::Conic(g) => gradient_brush(store, 2, FillKind::conic(g.spread), g),
    }
}

/// Lower a `Background` to a `ChromeRow`, sharing gradient lowering with shapes so chrome and shapes share one pool. Takes `bg` by reference: [`Background`] is deliberately not `Copy`.
pub(crate) fn background(store: &mut RecordStore, bg: &Background, ring: Stroke) -> ChromeRow {
    // Chrome's NaN gate, the second of two (the shape path's is `Shapes::add`), before lowering because `fill` interns its gradient.
    //
    // It sanitizes where the shape path drops: `chrome_table` keeps a row for `ClipMode::Rounded` even when the paint is no-op, so the stencil mask can read `corners`, and dropping it would leave the mask reading the NaN. Each field falls back to what its NaN meant; sanitizing before the hash keeps `ChromeRow.hash` honest.
    debug_assert!(
        !bg.has_nan(),
        "NaN in a Background — it degrades to no rounding and no paint: {bg:?}",
    );
    let fill_brush = if bg.fill.has_nan() || bg.is_noop() {
        &Brush::TRANSPARENT
    } else {
        &bg.fill
    };
    let fill = brush(store, fill_brush);
    let border = ShapeStroke {
        width: bg.border_inset(),
        ..ShapeStroke::from(if bg.border.has_nan() {
            Stroke::NONE
        } else {
            bg.border
        })
    };
    let corners = if bg.corners.has_nan() {
        Corners::ZERO
    } else {
        bg.corners
    };
    let shadow: LoweredShadow = if bg.shadow.has_nan() {
        Shadow::NONE.into()
    } else {
        bg.shadow.into()
    };
    let ring = ShapeStroke::from(ring);
    let has_ring = !ring.is_noop();
    // Fed as whole words from registers: a struct packed for one `write` is stored field by field and read back wider, a load the CPU can't forward.
    let brush = fill.hash_parts();
    let mut h = Hasher::new();
    h.write_u64(brush.payload);
    h.write_u64(corners.as_u64());
    border.hash_into(&mut h);
    ring.hash_into(&mut h);
    shadow.hash(&mut h);
    h.write_u8(brush.tag);
    let hash = ContentHash(h.finish());
    ChromeRow {
        fill,
        border,
        corners,
        shadow,
        hash,
        ring: has_ring,
    }
}

/// Lower a rounded or windowed rectangle onto the quad tier. Geometry is already in storage form; the fill interns through [`brush`], the pool [`background`] shares.
pub(crate) fn rect(
    store: &mut RecordStore,
    kind: RectKind,
    local_rect: Option<Rect>,
    corners: Corners,
    fill: &Brush,
    border: Stroke,
) -> ShapeRecord {
    ShapeRecord::Quad(QuadShape::Rect {
        kind,
        local_rect,
        corners,
        fill: brush(store, fill),
        border: ShapeStroke::from(border),
    })
}

/// Lower a mesh: copy vertices and indices into the store and freeze the bbox and content hash.
pub(crate) fn mesh(
    store: &mut RecordStore,
    mesh: &Mesh,
    local_rect: Option<Rect>,
    tint: RgbaF32,
) -> ShapeRecord {
    let staged = store.stage_mesh(mesh);
    ShapeRecord::Mesh {
        local_rect,
        tint: tint.into(),
        vertices: staged.vertices,
        indices: staged.indices,
        bbox: mesh.bbox(),
        content_hash: mesh.content_hash(),
    }
}

/// Lower a polyline into a `ShapeRecord::Polyline`: points into the store, colours multiplied by the stroke colour, then the content hash. Single-stroke shapes lower to `ShapeRecord::Curve` instead.
pub(crate) fn polyline(
    store: &mut RecordStore,
    points: &[Vec2],
    stroke: Stroke,
    colors: PolylineColors<'_>,
    cap: LineCap,
    join: LineJoin,
    bbox: Rect,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
        PolylineColors::Single => (ColorMode::Single, slice::from_ref(&RgbaF32::WHITE)),
        PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
        PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
    };

    // `Shapes::add` drops < 2-point polylines via `is_noop`, so a degenerate slice is a caller bug; colour cardinality is checked here since a query answers, it does not validate.
    debug_assert!(
        points.len() >= 2,
        "polyline with < 2 points reached lowering"
    );
    colors.assert_matches(points.len());
    // `bbox` was folded by `PolylineShape::new` so `Shapes::add` can screen before staging. Two passes beat one interleaved: the fold vectorizes alone and the copy becomes one `memcpy`.
    debug_assert!(
        !bbox.has_nan(),
        "NaN polyline point reached lowering — `Shapes::add` screens the bbox",
    );
    let staged = store.stage_polyline(points, color_slice, stroke.color);
    let lowered_colors = &store.polyline_colors[staged.colors.range()];

    let mut h = Hasher::new();
    for &point in points {
        point.hash_visual(&mut h);
    }
    h.pod_slice(lowered_colors);
    let style = u64::from(float_hash::canon_bits(stroke.width)) << 24
        | ((mode as u64) << 16)
        | ((cap as u64) << 8)
        | (join as u64);
    h.write_u64(style);
    let content_hash = h.finish();

    ShapeRecord::Polyline {
        width: stroke.width,
        color_mode: mode,
        cap,
        join,
        points: staged.points,
        colors: staged.colors,
        bbox,
        content_hash,
    }
}

/// Lower any [`CurveGeometry`] onto its [`CurveBasis`] plus a tight bbox. Tessellation is GPU-side at draw time.
///
/// Lines and quadratics reach the shader as cubics (exact promotion; a line's inner control points sit on the segment's thirds so `t` runs linearly and the composer keeps it one GPU instance). An arc keeps its own basis: the shader evaluates the exact circle.
pub(crate) fn curve(
    store: &mut RecordStore,
    geometry: CurveGeometry,
    style: CurveStyle,
) -> ShapeRecord {
    let CurveStyle { stroke, ramp, cap } = style;
    let bounded = match geometry {
        CurveGeometry::Line { a, b } => {
            let third = (b - a) / 3.0;
            cubic(a, a + third, b - third, b)
        }
        CurveGeometry::CubicBezier { p0, p1, p2, p3 } => cubic(p0, p1, p2, p3),
        CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
            let promoted = bezier::quadratic_to_cubic(p0, p1, p2);
            cubic(p0, promoted.c1, promoted.c2, p2)
        }
        CurveGeometry::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => {
            debug_assert!(
                sweep.abs() <= TAU + 1.0e-4,
                "Shape::arc sweep {sweep} exceeds a full circle (±2π)"
            );
            let a1 = start_angle + sweep;
            BoundedBasis {
                basis: CurveBasis::Arc {
                    center,
                    radius,
                    a0: start_angle,
                    a1,
                },
                bbox: arc::bbox(center, radius, start_angle, a1),
            }
        }
    };
    let ramp = match &ramp {
        None => CurveRamp::None,
        Some(ramp) => {
            let hash = grad_hash(3, ramp);
            let id = store.intern_gradient(
                hash,
                RecordedGradient {
                    axis: FillAxis::ZERO,
                    kind: FillKind::RAMP,
                    ramp: *ramp,
                },
            );
            CurveRamp::Interned { id, hash }
        }
    };
    curve_record(bounded, ShapeStroke::from(stroke), ramp, cap)
}

#[derive(Clone, Copy, Debug)]
struct BoundedBasis {
    basis: CurveBasis,
    bbox: Rect,
}

fn cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> BoundedBasis {
    BoundedBasis {
        basis: CurveBasis::Cubic { p0, p1, p2, p3 },
        bbox: bezier::cubic_bbox(p0, p1, p2, p3),
    }
}

/// The one `ShapeRecord::Curve` constructor, so the shared stroke fields are assembled once.
const fn curve_record(
    bounded: BoundedBasis,
    stroke: ShapeStroke,
    ramp: CurveRamp,
    cap: LineCap,
) -> ShapeRecord {
    let BoundedBasis { basis, bbox } = bounded;
    ShapeRecord::Curve {
        cap,
        basis,
        stroke,
        bbox,
        ramp,
    }
}

#[cfg(test)]
mod tests;
