//! Authoring → storage lowering: turns user-facing [`Shape`] inputs
//! and [`Background`] chrome into the [`ShapeRecord`] / [`ChromeRow`]
//! forms the tree stores. Bulk payload bytes (polyline points/colors,
//! gradients) append to the shared [`RecordStore`]; functions that
//! never touch the store (e.g. [`triangle`]) don't take it.
//!
//! Entry points: [`super::Shapes::add`] dispatches shapes here;
//! `Tree::open_node` calls [`background`] for chrome.
//!
//! [`Shape`]: crate::shape::Shape

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher as FxHasher;
use crate::forest::shapes::paint::{ChromeRow, LoweredShadow, ShapeBrush, ShapeStroke};
use crate::forest::shapes::record::ShapeRecord;
use crate::primitives::arc::arc_bbox;
use crate::primitives::background::Background;
use crate::primitives::bezier::{CurveBounds, cubic_bezier_bbox, quadratic_to_cubic};
use crate::primitives::brush::Brush;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::fill_wire::FillKind;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::stroke::Stroke;
use crate::record_store::{LoweredGradient, RecordStore};
use crate::renderer::gradient_atlas::handle::GradientAtlas;
use crate::renderer::render_buffer::curve::{HALF_FRINGE, MITER_LIMIT};
use crate::shape::{ColorMode, LineCap, LineJoin, PolylineColors};
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash::Hasher;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeInput<'a> {
    pub(crate) bg: &'a Background,
    pub(crate) store: &'a RecordStore,
    pub(crate) atlas: &'a GradientAtlas,
}

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
    let mut h = FxHasher::new();
    h.write_u8(tag);
    g.hash(&mut h);
    h.finish()
}

/// Lower a user-side `Brush` to the storage form: `Solid` stays
/// inline, gradients register their stops with the atlas, push a
/// [`LoweredGradient`] onto the store's pool, and return an indexing
/// `ShapeBrush::Gradient`. The pre-computed content hash is returned
/// alongside so the caller can stamp it into the `ShapeRecord` /
/// `ChromeRow` and keep their `Hash` impls context-free.
pub(crate) fn brush(store: &RecordStore, b: &Brush, atlas: &GradientAtlas) -> LoweredBrush {
    let (kind, axis, stops, interp, hash) = match b {
        Brush::Solid(c) => {
            return LoweredBrush {
                brush: ShapeBrush::Solid((*c).into()),
                hash: 0,
            };
        }
        Brush::Linear(g) => {
            let h = grad_hash(0, g);
            (FillKind::linear(g.spread), g.axis(), &g.stops, g.interp, h)
        }
        Brush::Radial(g) => {
            let h = grad_hash(1, g);
            (FillKind::radial(g.spread), g.axis(), &g.stops, g.interp, h)
        }
        Brush::Conic(g) => {
            let h = grad_hash(2, g);
            (FillKind::conic(g.spread), g.axis(), &g.stops, g.interp, h)
        }
    };
    let row = atlas.register_stops(stops, interp);
    let mut payloads = store.borrow_mut();
    let id = payloads.gradients.len() as u32;
    payloads.gradients.push(LoweredGradient { axis, row, kind });
    LoweredBrush {
        brush: ShapeBrush::Gradient(id),
        hash,
    }
}

/// Lower a user-facing `Background` to a `ChromeRow`. Same gradient
/// lowering as [`super::Shapes::add`] uses for `RoundedRect.fill`,
/// so chrome and shape paints share one pool. Takes `bg` by
/// reference — `Background` is 168 B and the recording chain
/// threads it through 4 functions; the per-field reads below copy
/// the small fields locally as needed.
pub(crate) fn background(store: &RecordStore, bg: &Background, atlas: &GradientAtlas) -> ChromeRow {
    let LoweredBrush {
        brush: fill,
        hash: fill_grad_hash,
    } = brush(store, &bg.fill, atlas);
    let stroke = ShapeStroke::from(&bg.stroke);
    let corners = bg.corners;
    let shadow: LoweredShadow = bg.shadow.into();
    // Canonical authoring hash: fold all inputs into one
    // `Hasher::pod` call. Hashing field-by-field via 5 separate
    // `Hasher::write*` calls (the prior shape) paid `hash_bytes`
    // setup + final `add_to_hash` 5 times — ~40 cycles of overhead
    // dominated `background`'s self-time (~0.5% of frame
    // total). Field order is layout-engineered to avoid internal
    // padding (u64s first → 2-align Pod structs → tag);
    // `padding_struct` fills the tail so `NoUninit` is sound.
    #[repr(C)]
    #[padding_struct::padding_struct]
    #[derive(Clone, Copy, bytemuck::NoUninit, bytemuck::Zeroable)]
    struct ChromeHashBytes {
        fill_payload: u64, // ColorF16-as-u64 (Solid) or fill_grad_hash (Gradient)
        corners_u64: u64,
        stroke: ShapeStroke,   // 10 B align 2
        shadow: LoweredShadow, // 18 B align 2
        fill_tag: u8,
    }
    let fill_payload: u64 = match fill {
        ShapeBrush::Solid(c) => c.as_u64(),
        ShapeBrush::Gradient(_) => fill_grad_hash,
    };
    let fill_tag: u8 = match fill {
        ShapeBrush::Solid(_) => 0,
        ShapeBrush::Gradient(_) => 1,
    };
    let packed = ChromeHashBytes {
        fill_payload,
        corners_u64: bytemuck::cast(corners),
        stroke,
        shadow,
        fill_tag,
        ..bytemuck::Zeroable::zeroed()
    };
    let mut h = FxHasher::new();
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

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: copy points and colors into the store,
/// compute the content hash. Only `Shape::Polyline` routes through
/// this — the one multi-segment stroke with interior joins; every
/// single-stroke shape (`Line`/beziers/`Arc`) lowers to a
/// `ShapeRecord::Curve`/`Arc` directly. Both render on the GPU
/// curve pipeline.
pub(crate) fn polyline(
    store: &RecordStore,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[Color]) = match &colors {
        PolylineColors::Single(c) => (ColorMode::Single, std::slice::from_ref(c)),
        PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
        PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
    };

    // `Shape::is_noop` drops < 2-point polylines before lowering
    // (`Shapes::add` gates on it), so a degenerate slice here is a
    // caller bug, not an input case.
    debug_assert!(
        points.len() >= 2,
        "polyline with < 2 points reached lowering"
    );
    let mut payloads = store.borrow_mut();
    let p_start = payloads.polyline_points.len() as u32;
    let c_start = payloads.polyline_colors.len() as u32;
    let (&first, rest) = points.split_first().unwrap();
    let mut lo = first;
    let mut hi = first;
    payloads.polyline_points.reserve(points.len());
    payloads.polyline_points.push(first);
    for &p in rest {
        payloads.polyline_points.push(p);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    payloads
        .polyline_colors
        .extend(color_slice.iter().map(|&c| ColorU8::from(c)));
    let bbox = inflate_stroke_bbox(lo, hi, width, cap, join);

    // Hash contract for polyline records: no variant tag needed —
    // polylines are the only shape lowering into this record, and
    // `compute_record_hash` writes the record tag anyway.
    let mut h = FxHasher::new();
    h.write(bytemuck::cast_slice(points));
    h.write(bytemuck::cast_slice(color_slice));
    let style = (width.to_bits() as u64) << 24
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
        points: Span::new(p_start, points.len() as u32),
        colors: Span::new(c_start, color_slice.len() as u32),
        bbox,
        content_hash,
    }
}

/// Lower a cubic bezier into a `ShapeRecord::Curve`. Tessellation
/// happens GPU-side at draw time — no CPU flattening, no per-curve
/// vertex/index allocation. The composer derives sub-instance count
/// from the post-transform control-polygon length. `brush` may be
/// `Brush::Solid` or `Brush::Linear` — the linear gradient samples
/// along the curve parameter `t` and its `angle` is ignored;
/// `Radial`/`Conic` panic at lowering (no meaningful axis on a
/// 1-D stroke).
pub(crate) fn cubic_bezier(
    store: &RecordStore,
    ctrl: [Vec2; 4],
    width: f32,
    brush: Brush,
    cap: LineCap,
    atlas: &GradientAtlas,
) -> ShapeRecord {
    assert_curve_brush(&brush);
    let lowered = self::brush(store, &brush, atlas);
    curve_inner(ctrl, width, lowered, cap)
}

/// Lower a quadratic bezier by promoting it to a cubic and going
/// through [`cubic_bezier`]'s path. Exact reparameterization:
/// `q1' = q0 + 2/3·(c - q0)`, `q2' = q2 + 2/3·(c - q2)`.
pub(crate) fn quadratic_bezier(
    store: &RecordStore,
    ctrl: [Vec2; 3],
    width: f32,
    brush: Brush,
    cap: LineCap,
    atlas: &GradientAtlas,
) -> ShapeRecord {
    assert_curve_brush(&brush);
    let [p0, c, p2] = ctrl;
    let cubic = quadratic_to_cubic(p0, c, p2);
    let lowered = self::brush(store, &brush, atlas);
    curve_inner([p0, cubic.c1, cubic.c2, p2], width, lowered, cap)
}

/// Lower a straight line as a degenerate cubic on the native GPU
/// stroke path. Inner control points sit on the segment's thirds,
/// so `B(t) = a + (b - a)·t` exactly — `t` (and thus a gradient
/// brush) runs linearly from `a` to `b`. The composer's flatness
/// fast-path keeps the collinear cubic a single GPU instance.
pub(crate) fn line(
    store: &RecordStore,
    a: Vec2,
    b: Vec2,
    width: f32,
    brush: Brush,
    cap: LineCap,
    atlas: &GradientAtlas,
) -> ShapeRecord {
    assert_curve_brush(&brush);
    let lowered = self::brush(store, &brush, atlas);
    let third = (b - a) / 3.0;
    curve_inner([a, a + third, b - third, b], width, lowered, cap)
}

/// Lower a circular arc into a [`ShapeRecord::Arc`]. Same native-GPU
/// stroke path as the béziers — no CPU flattening; the shader
/// evaluates the exact circle, so the record stores center/radius/
/// angles verbatim. `brush` follows the curve contract (`Solid` /
/// `Linear` sampled along the sweep; `Radial`/`Conic` rejected).
/// `|sweep| ≤ 2π` is debug-asserted: a longer sweep would repaint
/// pixels and double-blend a translucent stroke.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arc(
    store: &RecordStore,
    center: Vec2,
    radius: f32,
    start_angle: f32,
    sweep: f32,
    width: f32,
    brush: Brush,
    cap: LineCap,
    atlas: &GradientAtlas,
) -> ShapeRecord {
    assert_curve_brush(&brush);
    debug_assert!(
        sweep.abs() <= TAU + 1.0e-4,
        "Shape::Arc sweep {sweep} exceeds a full circle (±2π)"
    );
    let lowered = self::brush(store, &brush, atlas);
    let a1 = start_angle + sweep;
    let CurveBounds { lo, hi } = arc_bbox(center, radius, start_angle, a1);
    let bbox = padded_bbox(lo, hi, stroke_pad(width, cap));
    ShapeRecord::Arc {
        center,
        radius,
        a0: start_angle,
        a1,
        width,
        fill: lowered.brush,
        fill_grad_hash: lowered.hash,
        cap,
        bbox,
    }
}

/// Lower a triangle into a [`ShapeRecord::Triangle`]. Solid fill only —
/// gradients can't ride the reused quad-instance lanes, so `fill` is
/// `expect_solid`'d here (rejecting a gradient at the authoring boundary).
/// `bbox` is the owner-local AABB of `a`/`b`/`c` inflated by
/// `radius + AA fringe` (the SDF offsets the shape outward by `radius`;
/// the stroke is inner-edge and adds no outward reach), so damage and
/// clip-cull cover the rounded, antialiased extent. No store needed
/// (no gradient to register).
pub(crate) fn triangle(
    a: Vec2,
    b: Vec2,
    c: Vec2,
    radius: f32,
    fill: Brush,
    stroke: Stroke,
) -> ShapeRecord {
    let lo = a.min(b).min(c);
    let hi = a.max(b).max(c);
    let pad = radius.max(0.0) + HALF_FRINGE;
    let bbox = padded_bbox(lo, hi, pad);
    ShapeRecord::Triangle {
        a,
        b,
        c,
        radius,
        fill: fill.expect_solid().into(),
        stroke: ShapeStroke::from(stroke),
        bbox,
    }
}

/// GPU-stroked shapes (Line / beziers / Arc) accept `Brush::Solid` or
/// `Brush::Linear`. Radial and conic gradients project onto a 2-D
/// shape; there is no obvious projection onto a 1-D stroke (the chord
/// and the curve's bounding rect are both poor proxies), so we reject
/// at lowering rather than silently pick one. If a stroke-friendly
/// radial/conic interpretation shows up, lift this gate.
fn assert_curve_brush(brush: &Brush) {
    match brush {
        Brush::Solid(_) | Brush::Linear(_) => {}
        Brush::Radial(_) | Brush::Conic(_) => {
            panic!(
                "stroked shapes (Line / beziers / Arc): only Brush::Solid and Brush::Linear are supported"
            )
        }
    }
}

/// Conservative bbox padding for a GPU-stroked shape: half-width, plus
/// the cap extension (`Square`/`Round` reach `width/2` past each
/// endpoint along the local tangent — direction varies, so the
/// axis-aligned pad takes it on every side), plus the AA fringe.
fn stroke_pad(width: f32, cap: LineCap) -> f32 {
    let half = (width * 0.5).max(0.0);
    let cap_extent = match cap {
        LineCap::Butt => 0.0,
        LineCap::Square | LineCap::Round => half,
    };
    half + cap_extent + HALF_FRINGE
}

/// Build a `ShapeRecord::Curve` from cubic control points. The record
/// hash (`compute_record_hash`) covers the control points + width +
/// cap + brush directly — every input lives inline on the record, so
/// no lowering-time content hash is captured here.
fn curve_inner(ctrl: [Vec2; 4], width: f32, fill: LoweredBrush, cap: LineCap) -> ShapeRecord {
    let [p0, p1, p2, p3] = ctrl;

    let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
    let bbox = padded_bbox(lo, hi, stroke_pad(width, cap));
    ShapeRecord::Curve {
        p0,
        p1,
        p2,
        p3,
        width,
        fill: fill.brush,
        fill_grad_hash: fill.hash,
        cap,
        bbox,
    }
}

/// Inflate the centerline AABB `[lo, hi]` of a stroked polyline so it
/// conservatively covers the painted extent: stroke half-width on every
/// side, miter-limit slack at sharp joins (the shared [`MITER_LIMIT`]
/// the composer downgrades against), `Square` cap projection past
/// endpoints, and the AA fringe. Damage and per-shape clipping key on
/// this — undersizing here leaves miter spikes / square caps
/// unclipped/undamaged.
fn inflate_stroke_bbox(lo: Vec2, hi: Vec2, width: f32, cap: LineCap, join: LineJoin) -> Rect {
    let half = width * 0.5;
    let join_extent = if matches!(join, LineJoin::Miter) {
        half * MITER_LIMIT
    } else {
        half
    };
    let cap_extent = if matches!(cap, LineCap::Square) {
        half
    } else {
        0.0
    };
    let pad = join_extent.max(cap_extent) + HALF_FRINGE;
    padded_bbox(lo, hi, pad)
}

/// Axis-aligned bbox of `[lo, hi]` inflated by `pad` on every side.
/// Shared by the triangle / curve / stroke lowering paths above, which
/// differ only in how they derive `lo` / `hi` / `pad`.
fn padded_bbox(lo: Vec2, hi: Vec2, pad: f32) -> Rect {
    Rect {
        min: Vec2::new(lo.x - pad, lo.y - pad),
        size: Size {
            w: (hi.x - lo.x) + 2.0 * pad,
            h: (hi.y - lo.y) + 2.0 * pad,
        },
    }
}
