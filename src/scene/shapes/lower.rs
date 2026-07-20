//! Authoring → storage lowering: turns user-facing [`Shape`] inputs
//! and [`Background`] chrome into the [`ShapeRecord`] / [`ChromeRow`]
//! forms the tree stores. Bulk payload bytes (polyline points/colors,
//! gradients) append to the window's [`RecordStore`]; functions that
//! never touch the store (e.g. [`triangle`]) don't take it.
//!
//! Entry points: [`super::Shapes::add`] dispatches shapes here;
//! `Tree::open_node` calls [`background`] for chrome.
//!
//! [`Shape`]: crate::shape::Shape

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher as FxHasher;
use crate::primitives::approx;
use crate::primitives::arc::arc_bbox;
use crate::primitives::background::Background;
use crate::primitives::bezier::{CurveBounds, cubic_bezier_bbox, quadratic_to_cubic};
use crate::primitives::brush::{Brush, CurveBrush, LinearGradient};
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::fill_wire::FillKind;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::{RecordStore, RecordedGradient};
use crate::scene::shapes::paint::{ChromeRow, LoweredShadow, ShapeBrush, ShapeStroke};
use crate::scene::shapes::record::{ColorMode, ShapeRecord};
use crate::shape::PolylineColors;
use crate::shape::stroke_bounds::HALF_FRINGE;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash::Hasher;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeInput<'a> {
    pub(crate) bg: &'a Background,
    pub(crate) store: &'a RecordStore,
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

fn stored_gradient(store: &RecordStore, gradient: RecordedGradient, hash: u64) -> LoweredBrush {
    let id = store.payloads.borrow_mut().gradients.intern(hash, gradient);
    LoweredBrush {
        brush: ShapeBrush::Gradient(id),
        hash,
    }
}

fn solid_brush(color: Color) -> LoweredBrush {
    LoweredBrush {
        brush: ShapeBrush::Solid(color.into()),
        hash: 0,
    }
}

fn linear_brush(store: &RecordStore, gradient: &LinearGradient) -> LoweredBrush {
    stored_gradient(
        store,
        RecordedGradient {
            axis: gradient.axis(),
            kind: FillKind::linear(gradient.spread),
            stops: gradient.stops,
            interp: gradient.interp,
        },
        grad_hash(0, gradient),
    )
}

/// Lower a user-side `Brush` to the storage form: `Solid` stays
/// inline; gradients retain their content in the store and return an indexing
/// `ShapeBrush::Gradient`. The pre-computed content hash is returned
/// alongside so the caller can stamp it into the `ShapeRecord` /
/// `ChromeRow` and keep their `Hash` impls context-free.
pub(crate) fn brush(store: &RecordStore, b: &Brush) -> LoweredBrush {
    match b {
        Brush::Solid(color) => solid_brush(*color),
        Brush::Linear(gradient) => linear_brush(store, gradient),
        Brush::Radial(gradient) => stored_gradient(
            store,
            RecordedGradient {
                axis: gradient.axis(),
                kind: FillKind::radial(gradient.spread),
                stops: gradient.stops,
                interp: gradient.interp,
            },
            grad_hash(1, gradient),
        ),
        Brush::Conic(gradient) => stored_gradient(
            store,
            RecordedGradient {
                axis: gradient.axis(),
                kind: FillKind::conic(gradient.spread),
                stops: gradient.stops,
                interp: gradient.interp,
            },
            grad_hash(2, gradient),
        ),
    }
}

fn curve_brush(store: &RecordStore, brush: &CurveBrush) -> LoweredBrush {
    match brush {
        CurveBrush::Solid(color) => solid_brush(*color),
        CurveBrush::Linear(gradient) => linear_brush(store, gradient),
    }
}

/// Lower a user-facing `Background` to a `ChromeRow`. Same gradient
/// lowering as [`super::Shapes::add`] uses for `RoundedRect.fill`,
/// so chrome and shape paints share one pool. Takes `bg` by
/// reference — `Background` is 168 B and the recording chain
/// threads it through 4 functions; the per-field reads below copy
/// the small fields locally as needed.
pub(crate) fn background(store: &RecordStore, bg: &Background) -> ChromeRow {
    let LoweredBrush {
        brush: fill,
        hash: fill_grad_hash,
    } = brush(store, &bg.fill);
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
    let mut payloads = store.payloads.borrow_mut();
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
    let lowered_colors = &payloads.polyline_colors[c_start as usize..];
    let bbox = Rect::from_min_max(lo, hi);

    // Hash contract for polyline records: no variant tag needed —
    // polylines are the only shape lowering into this record, and
    // `compute_record_hash` writes the record tag anyway.
    let mut h = FxHasher::new();
    for &point in points {
        approx::hash_visual_vec2(point, &mut h);
    }
    h.write(bytemuck::cast_slice(lowered_colors));
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
        points: Span::new(p_start, points.len() as u32),
        colors: Span::new(c_start, color_slice.len() as u32),
        bbox,
        content_hash,
    }
}

/// Lower a cubic bezier into a `ShapeRecord::Curve`. Tessellation
/// happens GPU-side at draw time — no CPU flattening, no per-curve
/// vertex/index allocation. The composer derives sub-instance count
/// from the post-transform control-polygon length. A linear gradient samples
/// along the curve parameter `t`; its `angle` is ignored.
pub(crate) fn cubic_bezier(
    store: &RecordStore,
    ctrl: [Vec2; 4],
    width: f32,
    brush: CurveBrush,
    cap: LineCap,
) -> ShapeRecord {
    let lowered = curve_brush(store, &brush);
    curve_inner(ctrl, width, lowered, cap)
}

/// Lower a quadratic bezier by promoting it to a cubic and going
/// through [`cubic_bezier`]'s path. Exact reparameterization:
/// `q1' = q0 + 2/3·(c - q0)`, `q2' = q2 + 2/3·(c - q2)`.
pub(crate) fn quadratic_bezier(
    store: &RecordStore,
    ctrl: [Vec2; 3],
    width: f32,
    brush: CurveBrush,
    cap: LineCap,
) -> ShapeRecord {
    let [p0, c, p2] = ctrl;
    let cubic = quadratic_to_cubic(p0, c, p2);
    let lowered = curve_brush(store, &brush);
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
    brush: CurveBrush,
    cap: LineCap,
) -> ShapeRecord {
    let lowered = curve_brush(store, &brush);
    let third = (b - a) / 3.0;
    curve_inner([a, a + third, b - third, b], width, lowered, cap)
}

/// Lower a circular arc into a [`ShapeRecord::Arc`]. Same native-GPU
/// stroke path as the béziers — no CPU flattening; the shader
/// evaluates the exact circle, so the record stores center/radius/
/// angles verbatim. A linear gradient is sampled along the sweep.
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
    brush: CurveBrush,
    cap: LineCap,
) -> ShapeRecord {
    debug_assert!(
        sweep.abs() <= TAU + 1.0e-4,
        "Shape::Arc sweep {sweep} exceeds a full circle (±2π)"
    );
    let lowered = curve_brush(store, &brush);
    let a1 = start_angle + sweep;
    let CurveBounds { lo, hi } = arc_bbox(center, radius, start_angle, a1);
    let bbox = Rect::from_min_max(lo, hi);
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

/// Lower a triangle into a [`ShapeRecord::Triangle`].
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
    fill: Color,
    stroke: Stroke,
) -> ShapeRecord {
    let lo = a.min(b).min(c);
    let hi = a.max(b).max(c);
    let pad = radius.max(0.0) + HALF_FRINGE;
    let bbox = Rect::from_min_max(lo, hi).inflated(pad);
    ShapeRecord::Triangle {
        a,
        b,
        c,
        radius,
        fill: fill.into(),
        stroke: ShapeStroke::from(stroke),
        bbox,
    }
}

/// Build a `ShapeRecord::Curve` from cubic control points. The record
/// hash (`compute_record_hash`) covers the control points + width +
/// cap + brush directly — every input lives inline on the record, so
/// no lowering-time content hash is captured here.
fn curve_inner(ctrl: [Vec2; 4], width: f32, fill: LoweredBrush, cap: LineCap) -> ShapeRecord {
    let [p0, p1, p2, p3] = ctrl;

    let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
    let bbox = Rect::from_min_max(lo, hi);
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

#[cfg(test)]
mod tests {
    use super::brush;
    use crate::primitives::brush::{
        Brush, ConicGradient, Interp, LinearGradient, RadialGradient, Spread,
    };
    use crate::primitives::color::ColorU8;
    use crate::scene::record_store::{GradientId, RecordStore};
    use crate::scene::shapes::paint::ShapeBrush;
    use std::collections::HashSet;

    fn gradient_id(store: &RecordStore, value: &Brush) -> GradientId {
        match brush(store, value).brush {
            ShapeBrush::Gradient(id) => id,
            ShapeBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
        }
    }

    #[test]
    fn gradient_interning_identity_covers_geometry_kind_spread_and_interpolation() {
        let store = RecordStore::default();
        let colors = [ColorU8::hex(0x1a1a2e), ColorU8::hex(0x4c5cdb)];
        let base = LinearGradient::two_stop(0.25, colors[0], colors[1]);
        let first = gradient_id(&store, &Brush::Linear(base.clone()));
        assert_eq!(gradient_id(&store, &Brush::Linear(base.clone())), first);

        let changed_geometry = gradient_id(
            &store,
            &Brush::Linear(LinearGradient::two_stop(0.75, colors[0], colors[1])),
        );
        assert_ne!(changed_geometry, first);

        let mut mode_ids = HashSet::new();
        for spread in [Spread::Pad, Spread::Repeat, Spread::Reflect] {
            for interp in [Interp::Oklab, Interp::Linear] {
                let id = gradient_id(
                    &store,
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
            &store,
            &Brush::Radial(RadialGradient::two_stop_centered(colors[0], colors[1])),
        );
        let conic = gradient_id(
            &store,
            &Brush::Conic(ConicGradient::two_stop_centered(colors[0], colors[1])),
        );
        assert!(!mode_ids.contains(&radial));
        assert!(!mode_ids.contains(&conic));
        assert_ne!(radial, conic);
        assert_eq!(store.payloads.borrow().gradients.records.len(), 9);
    }
}
