//! Authoring-shape → `ShapeRecord` lowering. Private to the `shapes`
//! module: only `Shapes::add` calls these. Kept separate so the record
//! enum + its `Hash` impl (which downstream caches key on) can evolve
//! without dragging the curve-flattening / brush-extraction machinery
//! along.

use crate::common::hash::Hasher as FxHasher;
use crate::forest::shapes::{ShapePayloads, ShapeRecord};
use crate::layout::types::span::Span;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::{ColorMode, LineCap, LineJoin, PolylineColors};
use glam::Vec2;
use std::hash::Hasher;

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: validate `colors` length against
/// `points.len()`, copy both into the payload arenas, compute the
/// content hash. `Shape::Line` and `Shape::Polyline` both route
/// through this — one record path downstream.
pub(super) fn lower_polyline(
    payloads: &mut ShapePayloads,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) -> ShapeRecord {
    // Length contract is enforced at the authoring boundary by
    // `PolylineColors::assert_matches` in `Ui::add_shape`; the
    // `Shape::Line` path constructs `Single(color)` internally and is
    // unconstrained.
    // `Single(Brush)` extracts to a `Color` slot since slice-1 records
    // are still `Color`-typed; the local outlives the slice via the
    // outer binding.
    let single_color: Color;
    let (mode, color_slice): (ColorMode, &[Color]) = match colors {
        PolylineColors::Single(b) => {
            single_color = b.expect_solid();
            (ColorMode::Single, std::slice::from_ref(&single_color))
        }
        PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
        PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
    };

    let p_start = payloads.polyline_points.len() as u32;
    payloads.polyline_points.extend_from_slice(points);
    let c_start = payloads.polyline_colors.len() as u32;
    payloads.polyline_colors.extend_from_slice(color_slice);

    // Hash contract for polyline records: no variant tag. `Shape::Line`
    // and a 2-point `Shape::Polyline { Single(color) }` lower
    // byte-identically by design — sharing a hash is correct. Bezier
    // records tag themselves with `0xCB` + degree (see `lower_bezier`)
    // so curve-derived polylines can never collide with hand-authored
    // ones that happen to share the same flattened bytes.
    let mut h = FxHasher::new();
    h.write(bytemuck::cast_slice(points));
    h.write(bytemuck::cast_slice(color_slice));
    h.write_u32(width.to_bits());
    h.write_u8(mode as u8);
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    let content_hash = h.finish();

    // Owner-relative AABB computed once here so the encoder hot path
    // stays a straight `extend(map)`. Doesn't include cap-extension;
    // the composer inflates by the tessellator's outer-fringe offset
    // which already covers half-width (sufficient for Butt and a
    // tight upper bound for Square).
    let bbox = points_aabb(points);

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

/// AABB of a non-empty point slice. Returns the zero rect on empty
/// input — `Shape::is_noop` filters `points.len() < 2` upstream so
/// the empty branch is defensive, not hot.
fn points_aabb(points: &[Vec2]) -> Rect {
    let Some((&first, rest)) = points.split_first() else {
        return Rect::ZERO;
    };
    let (mut lo, mut hi) = (first, first);
    for p in rest {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    Rect {
        min: lo,
        size: Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    }
}

/// Control points for the unified bezier lowering — quadratic carries
/// three, cubic four. Just enough variant info to hash the right bytes
/// and tag the degree; flattening already happened before we get here
/// (different `flatten_*` per variant), so `lower_bezier` itself is
/// degree-agnostic past hashing.
pub(super) enum BezierInputs {
    Quadratic([Vec2; 3]),
    Cubic([Vec2; 4]),
}

/// Lower a flattened bezier (already in `payloads.bezier_scratch`)
/// into `ShapeRecord::Polyline`: copy points and track bbox in one
/// pass, push the single color, hash variant tag + control points +
/// style. `content_hash` covers control points + color + tolerance +
/// width + cap + join — the flattened output is derived from these
/// and shouldn't shift cache identity by itself. Solid color only
/// for now; t-parametric gradients (using `FlatPoint.t`) come later.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_bezier(
    payloads: &mut ShapePayloads,
    ctrl: BezierInputs,
    width: f32,
    color: Color,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    let Some((first, rest)) = payloads.bezier_scratch.split_first() else {
        // `flatten_*` always emits at least 2 points (start + end);
        // empty would mean a bezier with no endpoints. Defensive.
        unreachable!("flatten_{{cubic,quadratic}} always emits >= 2 points")
    };

    let p_start = payloads.polyline_points.len() as u32;
    let c_start = payloads.polyline_colors.len() as u32;
    let n = 1 + rest.len();

    payloads.polyline_colors.push(color);
    let mut lo = first.p;
    let mut hi = first.p;
    payloads.polyline_points.reserve(n);
    payloads.polyline_points.push(first.p);
    for fp in rest {
        payloads.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
    }

    // Hash contract: bezier-derived records tag with `0xCB` + degree
    // byte (0x01 cubic, 0x02 quadratic), so they can never collide
    // with `lower_polyline`'s untagged hash even if the flattened
    // bytes happened to match a hand-authored polyline.
    let mut h = FxHasher::new();
    h.write_u8(0xCB);
    match ctrl {
        BezierInputs::Cubic(ps) => {
            h.write_u8(0x01);
            h.write(bytemuck::bytes_of(&ps));
        }
        BezierInputs::Quadratic(ps) => {
            h.write_u8(0x02);
            h.write(bytemuck::bytes_of(&ps));
        }
    }
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    h.write(bytemuck::bytes_of(&color));
    let content_hash = h.finish();

    let bbox = Rect {
        min: lo,
        size: Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    };

    ShapeRecord::Polyline {
        width,
        color_mode: ColorMode::Single,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, 1),
        bbox,
        content_hash,
    }
}
