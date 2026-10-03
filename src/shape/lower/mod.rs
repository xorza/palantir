//! Authoring → storage lowering: turns user-facing [`Shape`] inputs
//! and [`Background`] chrome into the [`ShapeRecord`] / [`ChromeRow`]
//! forms the tree stores. Bulk payload bytes (polyline points/colors,
//! gradients) append to the window's [`RecordStore`]; functions that
//! never touch the store don't take it.
//!
//! **What lives here is what touches the store.** A shape whose record
//! is a repacking of its own fields builds it in its own `Lower` impl,
//! beside the type that knows the fields; a shape that has to *stage*
//! something — gradient stops, polyline points, mesh vertices — lowers
//! through a function here, so the `RecordStore` borrow stays on this
//! side of the authoring boundary and no builder reaches for it.
//!
//! Entry points: [`Shapes::add`](crate::shape::shapes::Shapes::add) dispatches shapes here;
//! `Tree::open_node` calls [`background`] for chrome.
//!
//! [`Shape`]: crate::shape::Shape

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::arc;
use crate::primitives::geometry::bezier;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::approx;
use crate::primitives::math::approx::FloatHash;
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
use std::hash::Hasher as _;

/// Stable content hash for a gradient kind or a curve ramp: discriminant
/// byte then the value's `Hash` impl (a gradient's hashes f32
/// canon-bits). Lets `ShapeRecord::Hash` stay context-free — the hash is
/// captured at lowering and rides in the `ShapeBrush::Gradient` or
/// `CurveRamp::Interned` beside its id, so downstream cache keys don't
/// need the store.
#[inline]
fn grad_hash<G: std::hash::Hash>(tag: u8, g: &G) -> u64 {
    let mut h = Hasher::new();
    h.write_u8(tag);
    g.hash(&mut h);
    h.finish()
}

/// Lower one gradient kind. `tag` and `kind` are the two things the
/// three kinds do not share — the discriminant byte that keeps their
/// content hashes apart, and the marker the shader branches on.
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

/// Lower a user-side `Brush` to the storage form: `Solid` stays
/// inline; gradients retain their content in the store and return a
/// `ShapeBrush::Gradient` holding the index and the content hash, which
/// keeps the `ShapeRecord` / `ChromeRow` hashes context-free.
pub(crate) fn brush(store: &mut RecordStore, b: &Brush) -> ShapeBrush {
    // No screen of its own: a gradient's geometry disappears into the
    // store behind a `GradientId`, so the decision has to be made before
    // the intern, and both callers make it — `Shapes::add` on the
    // authored shape, `background` below on the whole `Background`.
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

/// Lower a user-facing `Background` to a `ChromeRow`. Same gradient
/// lowering as [`Shapes::add`](crate::shape::shapes::Shapes::add) uses for rectangle fills,
/// so chrome and shape paints share one pool. Takes `bg` by
/// reference — the recording chain threads it through four functions
/// and [`Background`] is deliberately not `Copy`; the per-field reads
/// below copy the small fields locally as needed.
pub(crate) fn background(store: &mut RecordStore, bg: &Background) -> ChromeRow {
    // **Chrome's NaN gate**, and the second of the crate's two — the
    // shape path's is `Shapes::add`. It runs here for the same reason
    // that one runs before lowering: `fill` interns its gradient into the
    // store, so a broken one has to be caught while it is still in hand.
    //
    // It sanitizes where the shape path drops, because a chrome row has
    // *two* consumers. `chrome_table` deliberately keeps a row for
    // `ClipMode::Rounded` even when the paint is fully no-op, so the
    // encoder can read `corners` for the stencil mask — dropping the
    // chrome would fix the fill and leave the mask reading the NaN.
    //
    // Each field falls back to what its NaN already meant: no rounding,
    // no paint, no stroke, no shadow. Every one degrades safely — a
    // square background instead of a rounded one, a clip that still
    // clips. Sanitizing before the hash below keeps `ChromeRow.hash`
    // agreeing with what actually paints.
    debug_assert!(
        !bg.has_nan(),
        "NaN in a Background — it degrades to no rounding and no paint: {bg:?}",
    );
    // A background that paints nothing is kept only for a rounded clip's
    // corners. Its fill lowers to transparent rather than interning a
    // gradient no pass draws, which is also what lets
    // `ChromeRow::paints_nothing` answer from the row alone.
    let fill_brush = if bg.fill.has_nan() || bg.is_noop() {
        &Brush::TRANSPARENT
    } else {
        &bg.fill
    };
    let fill = brush(store, fill_brush);
    let border = ShapeStroke::from(if bg.border.has_nan() {
        Stroke::ZERO
    } else {
        bg.border
    });
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
    // Canonical authoring hash: fold all inputs into one
    // `Hasher::pod` call. Five separate `Hasher::write*` calls pay
    // `hash_bytes` setup + final `add_to_hash` five times — ~40 cycles
    // that dominate `background`'s self-time (~0.5% of frame total).
    // Field order is layout-engineered to avoid internal
    // padding — descending alignment, u64s first, then the Pod
    // structs widest-aligned first, then the tag; `padding_struct`
    // fills the tail so `NoUninit` is sound.
    #[repr(C)]
    #[padding_struct::padding_struct]
    #[derive(Debug, Clone, Copy, bytemuck::NoUninit, bytemuck::Zeroable)]
    struct ChromeHashBytes {
        fill_payload: u64, // RgbaF16-as-u64 (Solid) or content hash (Gradient)
        corners_u64: u64,
        border: ShapeStroke,   // 12 B align 4
        shadow: LoweredShadow, // 18 B align 2
        fill_tag: u8,
    }
    let brush = fill.hash_parts();
    let packed = ChromeHashBytes {
        fill_payload: brush.payload,
        corners_u64: corners.as_u64(),
        border,
        shadow,
        fill_tag: brush.tag,
        ..bytemuck::Zeroable::zeroed()
    };
    let mut h = Hasher::new();
    h.pod(&packed);
    let hash = ContentHash(h.finish());
    ChromeRow {
        fill,
        border,
        corners,
        shadow,
        hash,
    }
}

/// Lower a rounded or windowed rectangle onto the quad tier. Every
/// geometry input is already in its storage form, so the only work is
/// the fill: it interns through [`brush`], the same pool [`background`]
/// draws from, so chrome and rectangle fills share one gradient set.
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

/// Lower a mesh: copy its vertices and indices into the store and
/// freeze the bbox and content hash the record carries.
///
/// Here rather than in `MeshShape::lower` because staging is the whole
/// of what it does — the builder held a `&Mesh` and the record holds two
/// spans into the store, and reaching for the store from the authoring
/// side is the coupling this module exists to keep on one side of the
/// line.
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

/// Lower a polyline authoring shape into a `ShapeRecord::Polyline`: copy
/// the points into the store, and the colours multiplied by the stroke
/// colour, then compute the content hash. Only `Shape::Polyline` routes through
/// this — the one multi-segment stroke with interior joins; every
/// single-stroke shape (`Line`/beziers/`Arc`) lowers to a
/// `ShapeRecord::Curve` directly, picking its [`CurveBasis`]. Both
/// render on the GPU curve pipeline.
pub(crate) fn polyline(
    store: &mut RecordStore,
    points: &[Vec2],
    stroke: Stroke,
    colors: PolylineColors<'_>,
    cap: LineCap,
    join: LineJoin,
    bbox: Rect,
) -> ShapeRecord {
    // `Single` stages one white multiplier, so every mode takes the same
    // multiply and the stroke colour lands exactly: `1.0 × c` is `c`.
    let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
        PolylineColors::Single => (ColorMode::Single, std::slice::from_ref(&RgbaF32::WHITE)),
        PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
        PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
    };

    // `Shape::is_noop` drops < 2-point polylines before lowering
    // (`Shapes::add` gates on it), so a degenerate slice here is a
    // caller bug, not an input case. Colour cardinality is the same kind
    // of contract, checked here beside it rather than from the no-op
    // query — a query answers, it does not validate.
    debug_assert!(
        points.len() >= 2,
        "polyline with < 2 points reached lowering"
    );
    colors.assert_matches(points.len());
    // `bbox` was folded by `PolylineShape::new`, which is what let
    // `Shapes::add` screen this shape before anything below staged a
    // byte. Two passes over `points` rather than one interleaved pass,
    // and it is the faster shape by ~3x past a handful of points: the
    // fold vectorizes when nothing else shares the loop, and the copy
    // below becomes one `memcpy` instead of per-point `push`es.
    debug_assert!(
        !bbox.has_nan(),
        "NaN polyline point reached lowering — `Shapes::add` screens the bbox",
    );
    let staged = store.stage_polyline(points, color_slice, stroke.color);
    let lowered_colors = &store.polyline_colors[staged.colors.range()];

    // Hash contract for polyline records: no variant tag needed —
    // polylines are the only shape lowering into this record, and
    // `compute_record_hash` writes the record tag anyway.
    let mut h = Hasher::new();
    for &point in points {
        point.hash_visual(&mut h);
    }
    h.pod_slice(lowered_colors);
    let style = (approx::canon_bits(stroke.width) as u64) << 24
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

/// Lower any [`CurveGeometry`] onto its [`CurveBasis`] plus a tight
/// bbox. One entry point rather than four, so the geometry's fields are
/// read where they live instead of being destructured into a positional
/// call and rebuilt on the other side.
///
/// Tessellation happens GPU-side at draw time — no CPU flattening, no
/// per-curve vertex/index allocation. The composer derives sub-instance
/// count from the post-transform control-polygon length. A ramp samples
/// along the curve parameter `t`.
///
/// Lines and quadratics reach the shader as cubics. A line's inner
/// control points sit on the segment's thirds, so `B(t) = a + (b - a)·t`
/// exactly and `t` runs linearly from `a` to `b`; the composer's
/// flatness fast-path keeps that collinear cubic a single GPU instance.
/// A quadratic's promotion is exact, not an approximation. An arc keeps
/// its own basis: the shader evaluates the exact circle, so
/// centre/radius/angles are stored verbatim and a ramp is sampled along
/// the sweep.
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
            // `|sweep| ≤ 2π`: a longer sweep would repaint pixels and
            // double-blend a translucent stroke.
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
            // Tag 3, past the gradient kinds' 0..=2 in `brush`, so a ramp
            // and a gradient over the same stops hash apart.
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

/// One curve's shader basis and the tight bbox of its trace — what
/// every geometry resolves to before the shared stroke fields join it.
#[derive(Clone, Copy, Debug)]
struct BoundedBasis {
    basis: CurveBasis,
    bbox: Rect,
}

/// The three geometries that reach the shader as cubics differ only in
/// how they arrive at these four control points.
fn cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> BoundedBasis {
    BoundedBasis {
        basis: CurveBasis::Cubic { p0, p1, p2, p3 },
        bbox: bezier::cubic_bbox(p0, p1, p2, p3),
    }
}

/// The one `ShapeRecord::Curve` constructor — both bases land here, so
/// the stroke fields they share are assembled in exactly one place.
/// The record hash (`compute_record_hash`) covers the basis + stroke +
/// cap directly; the only lowering-time hash it reads is the one an
/// interned `ramp` carries.
fn curve_record(
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
