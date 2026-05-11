use crate::common::hash::Hasher as FxHasher;
use crate::layout::types::span::Span;
use crate::primitives::bezier::{
    eval_color_cubic, eval_color_quadratic, flatten_cubic, flatten_quadratic, lerp_color,
};
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::{
    ColorMode, CubicBezierColors, LineCap, LineJoin, PolylineColors, QuadraticBezierColors, Shape,
    ShapePayloads, ShapeRecord,
};
use glam::Vec2;
use std::hash::Hasher as _;

/// Per-frame shape store for one [`crate::forest::tree::Tree`].
///
/// - `records` is the flat shape buffer; each node owns a contiguous
///   sub-range via `NodeRecord.shape_span`. The gaps between a node's
///   children's spans hold that node's direct shapes in record order,
///   which is what [`crate::forest::tree::TreeItems`] interleaves.
/// - `payloads` holds variable-length side-tables that record variants
///   (`Mesh` / `Polyline`) reference via inner `Span`s.
///
/// Cleared together per frame, capacity retained — same lifecycle as
/// the rest of the tree.
#[derive(Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    pub(crate) payloads: ShapePayloads,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.payloads.clear();
    }

    /// Lower a user-facing [`Shape`] into a [`ShapeRecord`]: passthrough
    /// for rect/text, curve flattening for beziers, span-stamping for
    /// the variable-length variants (polyline / mesh) whose payloads
    /// land in `self.payloads`.
    pub(crate) fn lower(&mut self, shape: Shape<'_>) -> ShapeRecord {
        match shape {
            Shape::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            },
            Shape::Line {
                a,
                b,
                width,
                color,
                cap,
                join,
            } => lower_polyline(
                &mut self.payloads,
                &[a, b],
                PolylineColors::Single(color),
                width,
                cap,
                join,
            ),
            Shape::Polyline {
                points,
                colors,
                width,
                cap,
                join,
            } => lower_polyline(&mut self.payloads, points, colors, width, cap, join),
            Shape::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                width,
                colors,
                cap,
                join,
                tolerance,
            } => lower_cubic_bezier(
                &mut self.payloads,
                p0,
                p1,
                p2,
                p3,
                width,
                colors,
                cap,
                join,
                tolerance,
            ),
            Shape::QuadraticBezier {
                p0,
                p1,
                p2,
                width,
                colors,
                cap,
                join,
                tolerance,
            } => lower_quadratic_bezier(
                &mut self.payloads,
                p0,
                p1,
                p2,
                width,
                colors,
                cap,
                join,
                tolerance,
            ),
            Shape::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => ShapeRecord::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let arena = &mut self.payloads.meshes;
                let v_start = arena.vertices.len() as u32;
                arena.vertices.extend_from_slice(&mesh.vertices);
                let i_start = arena.indices.len() as u32;
                arena.indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh.content_hash();
                ShapeRecord::Mesh {
                    local_rect,
                    tint,
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices: Span::new(i_start, mesh.indices.len() as u32),
                    content_hash,
                }
            }
        }
    }
}

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: validate `colors` length against
/// `points.len()`, copy both into the payload arenas, compute the
/// content hash. `Shape::Line` and `Shape::Polyline` both route
/// through this — one record path downstream.
fn lower_polyline(
    payloads: &mut ShapePayloads,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[Color]) = match colors {
        PolylineColors::Single(ref c) => (ColorMode::Single, std::slice::from_ref(c)),
        PolylineColors::PerPoint(cs) => {
            assert_eq!(
                cs.len(),
                points.len(),
                "Shape::Polyline PerPoint colors len {} != points len {}",
                cs.len(),
                points.len(),
            );
            (ColorMode::PerPoint, cs)
        }
        PolylineColors::PerSegment(cs) => {
            assert_eq!(
                cs.len() + 1,
                points.len(),
                "Shape::Polyline PerSegment colors len {} != points len - 1 ({})",
                cs.len(),
                points.len().saturating_sub(1),
            );
            (ColorMode::PerSegment, cs)
        }
    };

    let p_start = payloads.polyline_points.len() as u32;
    payloads.polyline_points.extend_from_slice(points);
    let c_start = payloads.polyline_colors.len() as u32;
    payloads.polyline_colors.extend_from_slice(color_slice);

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

/// Lower [`Shape::CubicBezier`] into `ShapeRecord::Polyline` by
/// flattening into the payloads' bezier scratch, copying points into
/// `polyline_points`, evaluating the color mode per-point into
/// `polyline_colors`, and stamping spans. `content_hash` covers the
/// *control points + colors + tolerance + width + cap + join* — the
/// flattened output is derived from these and shouldn't shift cache
/// identity by itself.
#[allow(clippy::too_many_arguments)]
fn lower_cubic_bezier(
    payloads: &mut ShapePayloads,
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    width: f32,
    colors: CubicBezierColors,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    payloads.bezier_scratch.clear();
    flatten_cubic(p0, p1, p2, p3, tolerance, &mut payloads.bezier_scratch);

    let p_start = payloads.polyline_points.len() as u32;
    let n = payloads.bezier_scratch.len();
    let mut lo = payloads.bezier_scratch[0].p;
    let mut hi = lo;
    payloads.polyline_points.reserve(n);
    for fp in &payloads.bezier_scratch {
        payloads.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
    }

    let c_start = payloads.polyline_colors.len() as u32;
    let mode = match colors {
        CubicBezierColors::Solid(c) => {
            payloads.polyline_colors.push(c);
            ColorMode::Single
        }
        CubicBezierColors::Gradient2(a, b) => {
            payloads.polyline_colors.reserve(n);
            for fp in &payloads.bezier_scratch {
                payloads.polyline_colors.push(lerp_color(a, b, fp.t));
            }
            ColorMode::PerPoint
        }
        CubicBezierColors::Gradient3(a, b, c) => {
            payloads.polyline_colors.reserve(n);
            for fp in &payloads.bezier_scratch {
                payloads
                    .polyline_colors
                    .push(eval_color_quadratic(a, b, c, fp.t));
            }
            ColorMode::PerPoint
        }
        CubicBezierColors::Gradient4(a, b, c, d) => {
            payloads.polyline_colors.reserve(n);
            for fp in &payloads.bezier_scratch {
                payloads
                    .polyline_colors
                    .push(eval_color_cubic(a, b, c, d, fp.t));
            }
            ColorMode::PerPoint
        }
    };
    let c_len = payloads.polyline_colors.len() as u32 - c_start;

    let mut h = FxHasher::new();
    // Tag the variant so a polyline with the same numeric bytes can't
    // hash-collide with a bezier-derived record.
    h.write_u8(0xCB);
    h.write(bytemuck::bytes_of(&p0));
    h.write(bytemuck::bytes_of(&p1));
    h.write(bytemuck::bytes_of(&p2));
    h.write(bytemuck::bytes_of(&p3));
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    hash_cubic_colors(&mut h, &colors);
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
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, c_len),
        bbox,
        content_hash,
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_quadratic_bezier(
    payloads: &mut ShapePayloads,
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    width: f32,
    colors: QuadraticBezierColors,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    payloads.bezier_scratch.clear();
    flatten_quadratic(p0, p1, p2, tolerance, &mut payloads.bezier_scratch);

    let p_start = payloads.polyline_points.len() as u32;
    let n = payloads.bezier_scratch.len();
    let mut lo = payloads.bezier_scratch[0].p;
    let mut hi = lo;
    payloads.polyline_points.reserve(n);
    for fp in &payloads.bezier_scratch {
        payloads.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
    }

    let c_start = payloads.polyline_colors.len() as u32;
    let mode = match colors {
        QuadraticBezierColors::Solid(c) => {
            payloads.polyline_colors.push(c);
            ColorMode::Single
        }
        QuadraticBezierColors::Gradient2(a, b) => {
            payloads.polyline_colors.reserve(n);
            for fp in &payloads.bezier_scratch {
                payloads.polyline_colors.push(lerp_color(a, b, fp.t));
            }
            ColorMode::PerPoint
        }
        QuadraticBezierColors::Gradient3(a, b, c) => {
            payloads.polyline_colors.reserve(n);
            for fp in &payloads.bezier_scratch {
                payloads
                    .polyline_colors
                    .push(eval_color_quadratic(a, b, c, fp.t));
            }
            ColorMode::PerPoint
        }
    };
    let c_len = payloads.polyline_colors.len() as u32 - c_start;

    let mut h = FxHasher::new();
    h.write_u8(0xCB);
    h.write_u8(0x02); // quadratic discriminant
    h.write(bytemuck::bytes_of(&p0));
    h.write(bytemuck::bytes_of(&p1));
    h.write(bytemuck::bytes_of(&p2));
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    hash_quadratic_colors(&mut h, &colors);
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
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, c_len),
        bbox,
        content_hash,
    }
}

fn hash_cubic_colors(h: &mut FxHasher, colors: &CubicBezierColors) {
    match colors {
        CubicBezierColors::Solid(c) => {
            h.write_u8(0);
            h.write(bytemuck::bytes_of(c));
        }
        CubicBezierColors::Gradient2(a, b) => {
            h.write_u8(1);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
        }
        CubicBezierColors::Gradient3(a, b, c) => {
            h.write_u8(2);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
        }
        CubicBezierColors::Gradient4(a, b, c, d) => {
            h.write_u8(3);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
            h.write(bytemuck::bytes_of(d));
        }
    }
}

fn hash_quadratic_colors(h: &mut FxHasher, colors: &QuadraticBezierColors) {
    match colors {
        QuadraticBezierColors::Solid(c) => {
            h.write_u8(0);
            h.write(bytemuck::bytes_of(c));
        }
        QuadraticBezierColors::Gradient2(a, b) => {
            h.write_u8(1);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
        }
        QuadraticBezierColors::Gradient3(a, b, c) => {
            h.write_u8(2);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
        }
    }
}
