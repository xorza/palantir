//! Authoring → storage lowering: turns user-facing [`Shape`] inputs
//! and [`Background`] chrome into the [`ShapeRecord`] / [`ChromeRow`]
//! forms the tree stores. Bulk payload bytes (polyline points/colors,
//! gradients) append to the window's [`RecordStore`]; functions that
//! never touch the store (e.g. [`solid_brush`]) don't take it.
//!
//! **What lives here is what touches the store.** A shape whose record
//! is a repacking of its own fields builds it in its own `Lower` impl,
//! beside the type that knows the fields; a shape that has to *stage*
//! something — gradient stops, polyline points, mesh vertices — lowers
//! through a function here, so the `RecordStore` borrow stays on this
//! side of the authoring boundary and no builder reaches for it.
//!
//! Entry points: [`super::Shapes::add`] dispatches shapes here;
//! `Tree::open_node` calls [`background`] for chrome.
//!
//! [`Shape`]: crate::shape::Shape

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::approx;
use crate::primitives::approx::FloatHash;
use crate::primitives::arc;
use crate::primitives::background::Background;
use crate::primitives::bezier;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::mesh::Mesh;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::shapes::paint::{
    ChromeRow, CurveBasis, LoweredShadow, QuadShape, ShapeBrush, ShapeStroke,
};
use crate::scene::shapes::record::{ColorMode, ShapeRecord};
use crate::shape::curve::{CurveGeometry, CurveStroke};
use crate::shape::polyline::PolylineColors;
use crate::shape::rect::RectKind;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash::Hasher as _;

/// Result of lowering a user-side `Brush`. `brush` is the storage form
/// (`Solid` inline or `Gradient(id)` indexing into the store's
/// gradient pool); `hash` is the pre-computed content hash so the
/// caller can stamp it into a `ShapeRecord` / `ChromeRow` without
/// threading the store into their `Hash` impls. `hash == 0` for
/// `Solid` (no gradient payload to identify).
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredBrush {
    pub(crate) brush: ShapeBrush,
    pub(crate) hash: u64,
}

/// Stable content hash for a gradient variant: discriminant byte
/// then the gradient's `Hash` impl (which hashes f32 canon-bits).
/// Lets `ShapeRecord::Hash` stay context-free — we capture the hash
/// at lowering and stamp it on the record alongside the
/// `GradientId`, so downstream cache keys don't need the store.
#[inline]
fn grad_hash<G: std::hash::Hash>(tag: u8, g: &G) -> u64 {
    let mut h = Hasher::new();
    h.write_u8(tag);
    g.hash(&mut h);
    h.finish()
}

fn stored_gradient(store: &mut RecordStore, gradient: RecordedGradient, hash: u64) -> LoweredBrush {
    let id = store.intern_gradient(hash, gradient);
    LoweredBrush {
        brush: ShapeBrush::Gradient(id),
        hash,
    }
}

fn solid_brush(color: RgbaF32) -> LoweredBrush {
    LoweredBrush {
        brush: ShapeBrush::Solid(color.into()),
        hash: 0,
    }
}

/// Lower one gradient kind. `tag` and `kind` are the two things the
/// three kinds do not share — the discriminant byte that keeps their
/// content hashes apart, and the marker the shader branches on.
fn gradient_brush<G: GradientGeometry>(
    store: &mut RecordStore,
    tag: u8,
    kind: FillKind,
    gradient: &Gradient<G>,
) -> LoweredBrush {
    stored_gradient(
        store,
        RecordedGradient {
            axis: gradient.axis(),
            kind,
            stops: gradient.stops,
            interp: gradient.interp,
        },
        grad_hash(tag, gradient),
    )
}

/// Lower a user-side `Brush` to the storage form: `Solid` stays
/// inline; gradients retain their content in the store and return an indexing
/// `ShapeBrush::Gradient`. The pre-computed content hash is returned
/// alongside so the caller can stamp it into the `ShapeRecord` /
/// `ChromeRow` and keep their `Hash` impls context-free.
pub(crate) fn brush(store: &mut RecordStore, b: &Brush) -> LoweredBrush {
    // No screen of its own: a gradient's geometry disappears into the
    // store behind a `GradientId`, so the decision has to be made before
    // the intern, and both callers make it — `Shapes::add` on the
    // authored shape, `background` below on the whole `Background`.
    debug_assert!(
        !b.has_nan(),
        "NaN gradient geometry reached lowering: {b:?}"
    );
    match b {
        Brush::Solid(color) => solid_brush(*color),
        Brush::Linear(g) => gradient_brush(store, 0, FillKind::linear(g.spread), g),
        Brush::Radial(g) => gradient_brush(store, 1, FillKind::radial(g.spread), g),
        Brush::Conic(g) => gradient_brush(store, 2, FillKind::conic(g.spread), g),
    }
}

/// Lower a user-facing `Background` to a `ChromeRow`. Same gradient
/// lowering as [`super::Shapes::add`] uses for rectangle fills,
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
    let fill_brush = if bg.fill.has_nan() {
        &Brush::TRANSPARENT
    } else {
        &bg.fill
    };
    let LoweredBrush {
        brush: fill,
        hash: fill_grad_hash,
    } = brush(store, fill_brush);
    let stroke = ShapeStroke::from(if bg.stroke.has_nan() {
        Stroke::ZERO
    } else {
        bg.stroke
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
    // `Hasher::pod` call. Hashing field-by-field via 5 separate
    // `Hasher::write*` calls (the prior shape) paid `hash_bytes`
    // setup + final `add_to_hash` 5 times — ~40 cycles of overhead
    // dominated `background`'s self-time (~0.5% of frame
    // total). Field order is layout-engineered to avoid internal
    // padding — descending alignment, u64s first, then the Pod
    // structs widest-aligned first, then the tag; `padding_struct`
    // fills the tail so `NoUninit` is sound.
    #[repr(C)]
    #[padding_struct::padding_struct]
    #[derive(Debug, Clone, Copy, bytemuck::NoUninit, bytemuck::Zeroable)]
    struct ChromeHashBytes {
        fill_payload: u64, // RgbaF16-as-u64 (Solid) or fill_grad_hash (Gradient)
        corners_u64: u64,
        stroke: ShapeStroke,   // 12 B align 4
        shadow: LoweredShadow, // 18 B align 2
        fill_tag: u8,
    }
    let brush = fill.hash_parts(fill_grad_hash);
    let packed = ChromeHashBytes {
        fill_payload: brush.payload,
        corners_u64: corners.as_u64(),
        stroke,
        shadow,
        fill_tag: brush.tag,
        ..bytemuck::Zeroable::zeroed()
    };
    let mut h = Hasher::new();
    h.pod(&packed);
    let hash = ContentHash(h.finish());
    ChromeRow {
        fill,
        stroke,
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
    stroke: Stroke,
) -> ShapeRecord {
    let lowered = brush(store, fill);
    ShapeRecord::Quad(QuadShape::Rect {
        kind,
        local_rect,
        corners,
        fill: lowered.brush,
        stroke: ShapeStroke::from(stroke),
        fill_grad_hash: lowered.hash,
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

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: copy points and colors into the store,
/// compute the content hash. Only `Shape::Polyline` routes through
/// this — the one multi-segment stroke with interior joins; every
/// single-stroke shape (`Line`/beziers/`Arc`) lowers to a
/// `ShapeRecord::Curve` directly, picking its [`CurveBasis`]. Both
/// render on the GPU curve pipeline.
pub(crate) fn polyline(
    store: &mut RecordStore,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
    bbox: Rect,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
        PolylineColors::Single(c) => (ColorMode::Single, std::slice::from_ref(c)),
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
    let staged = store.stage_polyline(points, color_slice);
    let lowered_colors = &store.polyline_colors[staged.colors.range()];

    // Hash contract for polyline records: no variant tag needed —
    // polylines are the only shape lowering into this record, and
    // `compute_record_hash` writes the record tag anyway.
    let mut h = Hasher::new();
    for &point in points {
        point.hash_visual(&mut h);
    }
    h.pod_slice(lowered_colors);
    let style = (approx::canon_bits(width) as u64) << 24
        | ((mode as u64) << 16)
        | ((cap as u64) << 8)
        | (join as u64);
    h.write_u64(style);
    let content_hash = h.finish();

    ShapeRecord::Polyline {
        width,
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
/// count from the post-transform control-polygon length. A linear
/// gradient samples along the curve parameter `t`; its `angle` is
/// ignored.
///
/// Lines and quadratics reach the shader as cubics. A line's inner
/// control points sit on the segment's thirds, so `B(t) = a + (b - a)·t`
/// exactly and `t` runs linearly from `a` to `b`; the composer's
/// flatness fast-path keeps that collinear cubic a single GPU instance.
/// A quadratic's promotion is exact, not an approximation. An arc keeps
/// its own basis: the shader evaluates the exact circle, so
/// centre/radius/angles are stored verbatim and a linear gradient is
/// sampled along the sweep.
pub(crate) fn curve(
    store: &mut RecordStore,
    geometry: CurveGeometry,
    stroke: CurveStroke,
) -> ShapeRecord {
    let CurveStroke {
        width,
        brush: paint,
        cap,
    } = stroke;
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
    curve_record(bounded, width, brush(store, paint.as_brush()), cap)
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
/// The record hash (`compute_record_hash`) covers the basis + width +
/// cap + brush directly; every input lives inline on the record, so no
/// lowering-time content hash is captured here.
fn curve_record(
    bounded: BoundedBasis,
    width: f32,
    fill: LoweredBrush,
    cap: LineCap,
) -> ShapeRecord {
    let BoundedBasis { basis, bbox } = bounded;
    ShapeRecord::Curve {
        basis,
        width,
        fill: fill.brush,
        fill_grad_hash: fill.hash,
        cap,
        bbox,
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::background::Background;
    use crate::primitives::color::RgbaF32;
    use crate::primitives::corners::Corners;
    use crate::primitives::nan::NanCheck;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::scene::shapes::lower::background;

    use super::brush;
    use crate::primitives::brush::Brush;
    use crate::primitives::brush::gradient::conic_geometry::ConicGradient;
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
    use crate::primitives::brush::gradient::{Interp, Spread};
    use crate::primitives::color::RgbaU8;
    use crate::scene::record_store::RecordStore;
    use crate::scene::record_store::recorded_gradients::GradientId;
    use crate::scene::shapes::paint::ShapeBrush;
    use std::collections::HashSet;

    fn gradient_id(store: &mut RecordStore, value: &Brush) -> GradientId {
        match brush(store, value).brush {
            ShapeBrush::Gradient(id) => id,
            ShapeBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
        }
    }

    /// A white fill with `corners`, the shape every chrome case here
    /// varies one field of.
    fn with_corners(corners: Corners) -> Background {
        Background {
            corners,
            ..Background::fill(RgbaF32::WHITE)
        }
    }

    /// The four ways a `Background` can carry a NaN, one per field.
    ///
    /// All four are covered because no no-op predicate owns the question
    /// for any of them: "the radius is NaN" is not a reason the
    /// background paints nothing, and `approx_zero` reports NaN as
    /// non-zero by design so a NaN cannot take the sharp-corner fast
    /// path.
    fn nan_backgrounds() -> [(&'static str, Background); 4] {
        [
            (
                "corners",
                with_corners(Corners::new(4.0, f32::NAN, 4.0, 4.0)),
            ),
            (
                "fill",
                Background::fill(RgbaF32::srgba(1.0, f32::NAN, 1.0, 1.0)),
            ),
            (
                "stroke",
                Background {
                    stroke: Stroke::solid(RgbaF32::WHITE, f32::NAN),
                    ..Background::fill(RgbaF32::WHITE)
                },
            ),
            (
                "shadow",
                Background {
                    shadow: Shadow {
                        color: RgbaF32::WHITE,
                        blur: f32::NAN,
                        ..Shadow::default()
                    },
                    ..Background::fill(RgbaF32::WHITE)
                },
            ),
        ]
    }

    /// The three gradient kinds hash apart even on identical stops and
    /// geometry, because [`gradient_brush`] folds a discriminant byte in
    /// before the geometry. Without it a linear and a radial over the
    /// same two stops would share a content hash, and a brush swap
    /// between them would raise no damage.
    #[test]
    fn the_three_gradient_kinds_hash_apart_on_identical_stops() {
        let mut store = RecordStore::default();
        let stops = [
            crate::primitives::brush::gradient::stops::Stop::new(0.0, RgbaF32::BLACK),
            crate::primitives::brush::gradient::stops::Stop::new(1.0, RgbaF32::WHITE),
        ];
        let centre = glam::Vec2::splat(0.5);
        let hashes = [
            brush(&mut store, &Brush::Linear(LinearGradient::new(0.0, stops))).hash,
            brush(
                &mut store,
                &Brush::Radial(RadialGradient::new(centre, centre, stops)),
            )
            .hash,
            brush(
                &mut store,
                &Brush::Conic(ConicGradient::new(centre, 0.0, stops)),
            )
            .hash,
        ];
        let distinct: HashSet<u64> = hashes.iter().copied().collect();
        assert_eq!(distinct.len(), 3, "{hashes:?}");
    }

    /// A sane background reaches the row unchanged, and every field it
    /// carries reaches the hash — which is what makes the sanitizing
    /// below a change of behaviour rather than a no-op.
    #[test]
    fn background_lowering_keeps_an_authored_field() {
        let mut store = RecordStore::default();
        let sane = Corners::all(6.0);
        let kept = background(&mut store, &with_corners(sane));
        assert_eq!(kept.corners, sane);
        assert_ne!(
            kept.hash,
            background(&mut store, &with_corners(Corners::ZERO)).hash,
            "corners must still reach the chrome hash",
        );
    }

    /// Chrome is the paint path `Shapes::add` never sees, so `background`
    /// is its NaN gate — and it sanitizes where the shape path drops,
    /// because `chrome_table` keeps a row for `ClipMode::Rounded` even
    /// when the paint is no-op. A dropped background would fix the fill
    /// and leave the stencil mask reading the NaN.
    ///
    /// **The claim is one both profiles keep: a NaN never reaches the
    /// row.** A debug build says so by asserting, a release build by
    /// falling each field back to what its NaN already meant, and the
    /// `catch_unwind` accepts either — the same shape
    /// `the_nan_gate_drops_every_shape_kind` pins the shape path with.
    #[test]
    fn a_nan_background_field_never_reaches_the_row() {
        let mut store = RecordStore::default();
        for (label, authored) in nan_backgrounds() {
            let Ok(row) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                background(&mut store, &authored)
            })) else {
                // The gate asserted, which is the loudest form of "did
                // not reach the row".
                continue;
            };
            assert!(
                !row.corners.has_nan()
                    && !row.stroke.has_nan()
                    && !row.shadow.has_nan()
                    && !row.fill.has_nan(),
                "a NaN {label} must not survive lowering",
            );
        }

        // A radius falls back to *no rounding* specifically, not merely
        // to something finite: that is what leaves a `ClipMode::Rounded`
        // stencil readable rather than clipping to a shape nobody chose.
        if let Ok(row) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            background(
                &mut store,
                &with_corners(Corners::new(4.0, f32::NAN, 4.0, 4.0)),
            )
        })) {
            assert!(row.corners.approx_zero(), "a radius collapses to none");
        }
    }

    #[test]
    fn gradient_interning_identity_covers_geometry_kind_spread_and_interpolation() {
        let mut store = RecordStore::default();
        let colors = [RgbaU8::hex(0x1a1a2e), RgbaU8::hex(0x4c5cdb)];
        let base = LinearGradient::two_stop(0.25, colors[0], colors[1]);
        let first = gradient_id(&mut store, &Brush::Linear(base.clone()));
        assert_eq!(gradient_id(&mut store, &Brush::Linear(base.clone())), first);

        let changed_geometry = gradient_id(
            &mut store,
            &Brush::Linear(LinearGradient::two_stop(0.75, colors[0], colors[1])),
        );
        assert_ne!(changed_geometry, first);

        let mut mode_ids = HashSet::new();
        for spread in [Spread::Pad, Spread::Repeat, Spread::Reflect] {
            for interp in [Interp::Oklab, Interp::Linear] {
                let id = gradient_id(
                    &mut store,
                    &Brush::Linear(base.clone().with_spread(spread).with_interp(interp)),
                );
                assert!(
                    mode_ids.insert(id),
                    "spread/interpolation pair reused another pair's gradient id",
                );
            }
        }
        assert_eq!(mode_ids.len(), 6);

        let radial = gradient_id(
            &mut store,
            &Brush::Radial(RadialGradient::two_stop_centered(colors[0], colors[1])),
        );
        let conic = gradient_id(
            &mut store,
            &Brush::Conic(ConicGradient::two_stop_centered(colors[0], colors[1])),
        );
        assert!(!mode_ids.contains(&radial));
        assert!(!mode_ids.contains(&conic));
        assert_ne!(radial, conic);
        assert_eq!(store.gradients.records.len(), 9);
    }
}
