use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::shape::{ColorMode, LineCap, LineJoin};
use glam::Vec2;

const HALF_FRINGE: f32 = 0.5;
/// SVG default. Beyond this the miter would project a long spike,
/// so we fall back to bevel geometry at the join instead.
const MITER_LIMIT: f32 = 4.0;
/// Cross-section block size: 4 verts per cross-section (outer
/// fringe, inner edge, inner edge, outer fringe).
const BLOCK: u16 = 4;

/// Tessellate a stroked polyline as a fringe-AA mesh.
///
/// Inputs are in **physical px** — composer applies the active
/// transform + DPI scale to `points` and `width_phys` before
/// calling. Colors are premultiplied linear RGBA. `mode` picks
/// the color-storage interpretation and the vertex layout:
///
/// - [`ColorMode::Single`] — `colors.len() == 1`. Same color on
///   every cross-section.
/// - [`ColorMode::PerPoint`] — `colors.len() == points.len()`. GPU
///   lerps between adjacent cross-sections, giving a smooth
///   gradient along the stroke.
/// - [`ColorMode::PerSegment`] — `colors.len() == points.len() - 1`.
///   Each segment paints as a solid block; interior cross-sections
///   duplicate so colors don't bleed at joins. Join chrome (bevel
///   bridge / round fan / concave fill) and round caps paint with
///   the average of the two adjacent segments' colors.
///
/// **Hairline behavior.** For `width_phys < 1`, geometry freezes
/// at 1 physical px wide and per-vertex colors are alpha-scaled by
/// `width_phys` (premultiplied → rgb and alpha by the same
/// factor). A 0.3-px line paints as a 1-px line at α=0.3 of each
/// vertex's input color.
///
/// **Joins.** Miter clamped to [`MITER_LIMIT`] (falls back to bevel
/// geometry past the limit); Bevel and Round selectable per stroke.
/// **Caps.** Butt, Square, and Round selectable per stroke.
///
/// **Indexing.** Indices are pushed **0-based** to the verts this
/// call emits — composer captures `phys_v_start = out_verts.len()`
/// before calling and passes it as `MeshDraw.vertices.start`,
/// which becomes the wgpu `base_vertex`. Multiple calls into the
/// same vecs concatenate independent index blocks.
#[derive(Clone, Copy)]
pub(crate) struct StrokeStyle {
    pub(crate) mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    pub(crate) width_phys: f32,
}

pub(crate) fn tessellate_polyline_aa(
    points: &[Vec2],
    colors: &[Color],
    style: StrokeStyle,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    if points.len() < 2 {
        return;
    }
    debug_assert!(matches_mode(points.len(), colors.len(), style.mode));

    let half_geom = (style.width_phys * 0.5).max(HALF_FRINGE);
    let geo = Geo {
        outer_offset: half_geom + HALF_FRINGE,
        inner_offset: half_geom,
        alpha_scale: style.width_phys.clamp(0.0, 1.0),
        cap: style.cap,
        join: style.join,
    };
    match style.mode {
        ColorMode::Single | ColorMode::PerPoint => {
            emit_simple(points, colors, style.mode, geo, out_verts, out_indices);
        }
        ColorMode::PerSegment => {
            emit_per_segment(points, colors, geo, out_verts, out_indices);
        }
    }
}

/// Geometry + style parameters shared by both emit paths. Pre-
/// computed in [`tessellate_polyline_aa`]'s setup so the inner
/// loops just read the resolved values.
#[derive(Clone, Copy)]
struct Geo {
    outer_offset: f32,
    inner_offset: f32,
    alpha_scale: f32,
    cap: LineCap,
    join: LineJoin,
}

impl Geo {
    /// Cap-extension distance along the segment direction at
    /// endpoints. Only `LineCap::Square` extends; Butt and Round
    /// leave the cross-section at the endpoint and use their
    /// fan-emission paths for visible chrome.
    #[inline]
    fn cap_extension(&self) -> f32 {
        match self.cap {
            LineCap::Square => self.inner_offset,
            LineCap::Butt | LineCap::Round => 0.0,
        }
    }

    /// `true` when this interior point must emit dual cross-sections
    /// (bevel or round join) instead of the single miter
    /// cross-section.
    #[inline]
    fn needs_dual_section(&self, normal_prev: Vec2, normal_next: Vec2) -> bool {
        match self.join {
            LineJoin::Miter => is_sharp_join(normal_prev, normal_next),
            LineJoin::Bevel | LineJoin::Round => true,
        }
    }
}

fn matches_mode(points_len: usize, colors_len: usize, mode: ColorMode) -> bool {
    match mode {
        ColorMode::Single => colors_len == 1,
        ColorMode::PerPoint => colors_len == points_len,
        ColorMode::PerSegment => colors_len + 1 == points_len,
    }
}

/// Single + PerPoint emission: one cross-section per input point
/// for non-sharp joins; two cross-sections (a bevel) when the miter
/// factor would exceed [`MITER_LIMIT`].
fn emit_simple(
    points: &[Vec2],
    colors: &[Color],
    mode: ColorMode,
    geo: Geo,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = points.len();
    let call_start_verts = out_verts.len();
    let mut prev_offset: u16 = 0;
    let mut prev_was_dual = false;
    let mut prev_seg_normal: Option<Vec2> = None;

    for i in 0..n {
        let next_seg_normal = if i + 1 < n {
            Some(seg_normal(points[i], points[i + 1]))
        } else {
            None
        };
        let is_dual = match (prev_seg_normal, next_seg_normal) {
            (Some(np), Some(nn)) => geo.needs_dual_section(np, nn),
            _ => false,
        };
        let current_offset = current_offset(out_verts.len(), call_start_verts);
        let color = scale_alpha(
            match mode {
                ColorMode::Single => colors[0],
                ColorMode::PerPoint => colors[i],
                ColorMode::PerSegment => unreachable!(),
            },
            geo.alpha_scale,
        );

        // 1. Cross-section verts.
        match (prev_seg_normal, next_seg_normal) {
            (Some(np), Some(nn)) if is_dual => {
                push_cross_section(points[i], np, 1.0, geo, color, out_verts);
                push_cross_section(points[i], nn, 1.0, geo, color, out_verts);
            }
            (Some(np), Some(nn)) => {
                let m = miter_bisector(np, nn);
                push_cross_section(points[i], m.dir, m.ext, geo, color, out_verts);
            }
            (None, Some(nn)) => {
                let p = points[i] - tangent_of(nn) * geo.cap_extension();
                push_cross_section(p, nn, 1.0, geo, color, out_verts);
            }
            (Some(np), None) => {
                let p = points[i] + tangent_of(np) * geo.cap_extension();
                push_cross_section(p, np, 1.0, geo, color, out_verts);
            }
            (None, None) => unreachable!("polyline length < 2 short-circuits earlier"),
        }

        // 2. Strip indices for segment (i-1, i).
        if i > 0 {
            let leading = prev_offset + if prev_was_dual { BLOCK } else { 0 };
            push_strip_indices(leading, current_offset, out_indices);
        }

        // 3. Join chrome at this point.
        if is_dual {
            let np = prev_seg_normal.unwrap();
            let nn = next_seg_normal.unwrap();
            push_join_chrome(
                points[i],
                current_offset,
                current_offset + BLOCK,
                np,
                nn,
                geo,
                color,
                call_start_verts,
                out_verts,
                out_indices,
            );
        }

        // 4. Round cap fans at endpoints.
        if matches!(geo.cap, LineCap::Round) {
            if i == 0
                && let Some(nn) = next_seg_normal
            {
                push_round_cap(
                    points[i],
                    -tangent_of(nn),
                    geo,
                    color,
                    call_start_verts,
                    out_verts,
                    out_indices,
                );
            }
            if i == n - 1
                && let Some(np) = prev_seg_normal
            {
                push_round_cap(
                    points[i],
                    tangent_of(np),
                    geo,
                    color,
                    call_start_verts,
                    out_verts,
                    out_indices,
                );
            }
        }

        prev_offset = current_offset;
        prev_was_dual = is_dual;
        prev_seg_normal = next_seg_normal;
    }
}

/// Per-segment paints each segment in a solid block. Interior
/// cross-sections duplicate (one belonging to segment `i-1`, one
/// to segment `i`) so the strip between two cross-sections
/// belongs to a single segment and carries that segment's color
/// uniformly. Join chrome and round caps paint with the average
/// of the two adjacent segments' colors.
fn emit_per_segment(
    points: &[Vec2],
    colors: &[Color],
    geo: Geo,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = points.len();
    let call_start_verts = out_verts.len();
    let mut np = seg_normal(points[0], points[1]);

    // Start endpoint.
    let start_color = scale_alpha(colors[0], geo.alpha_scale);
    let p0 = points[0] - tangent_of(np) * geo.cap_extension();
    push_cross_section(p0, np, 1.0, geo, start_color, out_verts);
    if matches!(geo.cap, LineCap::Round) {
        push_round_cap(
            points[0],
            -tangent_of(np),
            geo,
            start_color,
            call_start_verts,
            out_verts,
            out_indices,
        );
    }
    let mut prev_block_offset: u16 = 0;

    for i in 1..n - 1 {
        let nn = seg_normal(points[i], points[i + 1]);
        let dual = geo.needs_dual_section(np, nn);
        let (trailing_normal, trailing_ext, leading_normal, leading_ext) = if dual {
            (np, 1.0, nn, 1.0)
        } else {
            let m = miter_bisector(np, nn);
            (m.dir, m.ext, m.dir, m.ext)
        };

        let trailing_color = scale_alpha(colors[i - 1], geo.alpha_scale);
        let leading_color = scale_alpha(colors[i], geo.alpha_scale);

        let trailing_offset = current_offset(out_verts.len(), call_start_verts);
        push_cross_section(
            points[i],
            trailing_normal,
            trailing_ext,
            geo,
            trailing_color,
            out_verts,
        );
        // Close segment (i-1, i): strip from prev_block_offset to trailing_offset.
        push_strip_indices(prev_block_offset, trailing_offset, out_indices);

        let leading_offset = current_offset(out_verts.len(), call_start_verts);
        push_cross_section(
            points[i],
            leading_normal,
            leading_ext,
            geo,
            leading_color,
            out_verts,
        );

        if dual {
            push_join_chrome(
                points[i],
                trailing_offset,
                leading_offset,
                np,
                nn,
                geo,
                avg_color(trailing_color, leading_color),
                call_start_verts,
                out_verts,
                out_indices,
            );
        }

        prev_block_offset = leading_offset;
        np = nn;
    }

    // End endpoint.
    let end_color = scale_alpha(colors[n - 2], geo.alpha_scale);
    let end_offset = current_offset(out_verts.len(), call_start_verts);
    let pl = points[n - 1] + tangent_of(np) * geo.cap_extension();
    push_cross_section(pl, np, 1.0, geo, end_color, out_verts);
    push_strip_indices(prev_block_offset, end_offset, out_indices);
    if matches!(geo.cap, LineCap::Round) {
        push_round_cap(
            points[n - 1],
            tangent_of(np),
            geo,
            end_color,
            call_start_verts,
            out_verts,
            out_indices,
        );
    }
}

/// Dispatch to bevel / round chrome plus the concave-side notch
/// fill. Shared between [`emit_simple`] and [`emit_per_segment`] so
/// the two paths can't drift on join geometry.
#[allow(clippy::too_many_arguments)]
fn push_join_chrome(
    center: Vec2,
    trailing_block: u16,
    leading_block: u16,
    normal_prev: Vec2,
    normal_next: Vec2,
    geo: Geo,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    match geo.join {
        LineJoin::Round => push_round_join(
            center,
            normal_prev,
            normal_next,
            geo,
            inner_color,
            call_start_verts,
            out_verts,
            out_indices,
        ),
        LineJoin::Bevel | LineJoin::Miter => push_bevel_bridge(
            center,
            trailing_block,
            leading_block,
            normal_prev,
            normal_next,
            inner_color,
            call_start_verts,
            out_verts,
            out_indices,
        ),
    }
    push_concave_fill(
        center,
        trailing_block,
        leading_block,
        normal_prev,
        normal_next,
        inner_color,
        call_start_verts,
        out_verts,
        out_indices,
    );
}

/// True iff the miter factor at this join would exceed
/// [`MITER_LIMIT`]. Antiparallel segments (`cos_half ≈ 0`) count
/// as sharp.
#[inline]
fn is_sharp_join(normal_prev: Vec2, normal_next: Vec2) -> bool {
    let sum = normal_prev + normal_next;
    let len_sq = sum.length_squared();
    if len_sq < 1e-6 {
        return true;
    }
    let bisector = sum / len_sq.sqrt();
    let cos_half = bisector.dot(normal_prev);
    cos_half < 1.0 / MITER_LIMIT
}

struct Miter {
    dir: Vec2,
    ext: f32,
}

/// Bisector direction + miter extension factor (unclamped). Caller
/// must have already determined this join is *not* sharp via
/// [`is_sharp_join`] — otherwise `ext` could be arbitrarily large.
#[inline]
fn miter_bisector(normal_prev: Vec2, normal_next: Vec2) -> Miter {
    let sum = normal_prev + normal_next;
    let dir = sum / sum.length();
    let cos_half = dir.dot(normal_prev);
    Miter {
        dir,
        ext: 1.0 / cos_half,
    }
}

/// Bridge the convex-side gap at a beveled join. The cross product
/// of the two normals picks the convex side: positive → CCW turn
/// → convex on `-normal` side (verts 2,3); negative → CW turn →
/// convex on `+normal` side (verts 0,1). Emits a center triangle
/// at `P` plus one fringe quad joining the inner-edge + outer-
/// fringe verts on the convex side.
#[allow(clippy::too_many_arguments)]
fn push_bevel_bridge(
    center: Vec2,
    trailing_block: u16,
    leading_block: u16,
    normal_prev: Vec2,
    normal_next: Vec2,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let cross = normal_prev.perp_dot(normal_next);
    let (inner_off, outer_off) = if cross > 0.0 { (2, 3) } else { (1, 0) };
    let t_inner = trailing_block + inner_off;
    let t_outer = trailing_block + outer_off;
    let l_inner = leading_block + inner_off;
    let l_outer = leading_block + outer_off;
    // Center vert closes the wedge between corner point P and the
    // bridge's inner edge — without it the strip end-edges leave a
    // pinhole at P.
    let center_idx = current_offset(out_verts.len(), call_start_verts);
    out_verts.push(MeshVertex {
        pos: center,
        color: inner_color,
    });
    out_indices.extend_from_slice(&[center_idx, t_inner, l_inner]);
    out_indices.extend_from_slice(&[t_inner, t_outer, l_outer, t_inner, l_outer, l_inner]);
}

/// Concave-side fill at a dual join. The two adjacent strips
/// terminate their concave-inner edges at different positions
/// (each perpendicular to its own segment), leaving a notch on the
/// inside of the corner. Close it with a triangle anchored at `P`
/// plus the two concave inner verts. The outer-fringe gap stays
/// uncovered (AA gradient → invisible at typical zoom).
#[allow(clippy::too_many_arguments)]
fn push_concave_fill(
    center: Vec2,
    trailing_block: u16,
    leading_block: u16,
    normal_prev: Vec2,
    normal_next: Vec2,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let cross = normal_prev.perp_dot(normal_next);
    let inner_off: u16 = if cross > 0.0 { 1 } else { 2 };
    let t_concave = trailing_block + inner_off;
    let l_concave = leading_block + inner_off;
    let center_idx = current_offset(out_verts.len(), call_start_verts);
    out_verts.push(MeshVertex {
        pos: center,
        color: inner_color,
    });
    out_indices.extend_from_slice(&[center_idx, t_concave, l_concave]);
}

/// Number of fan slices for a round cap or join. Scales with the
/// stroke's geometry-half so a 1 px hairline cap is the cheap
/// minimum and a fat stroke gets a smooth arc.
#[inline]
fn round_segments(inner_offset: f32) -> u16 {
    (inner_offset.ceil() as u16 * 2).clamp(4, 16)
}

/// Round-cap fan: half-disc centered at `center`, opening toward
/// `outward`.
#[allow(clippy::too_many_arguments)]
fn push_round_cap(
    center: Vec2,
    outward: Vec2,
    geo: Geo,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = round_segments(geo.inner_offset);
    push_round_fan(
        center,
        outward,
        std::f32::consts::FRAC_PI_2,
        n,
        geo,
        inner_color,
        call_start_verts,
        out_verts,
        out_indices,
    );
}

/// Round join: arc fan filling the convex-side wedge between the
/// two segments.
#[allow(clippy::too_many_arguments)]
fn push_round_join(
    center: Vec2,
    normal_prev: Vec2,
    normal_next: Vec2,
    geo: Geo,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let cross = normal_prev.perp_dot(normal_next);
    let (convex_prev, convex_next) = if cross > 0.0 {
        (-normal_prev, -normal_next)
    } else {
        (normal_prev, normal_next)
    };
    let sum = convex_prev + convex_next;
    let sum_len_sq = sum.length_squared();
    // Antiparallel (180° fold) → bisector degenerate; pick a
    // perpendicular and use a full half-disc.
    let (bisector, half_angle) = if sum_len_sq < 1e-6 {
        (
            Vec2::new(-convex_prev.y, convex_prev.x),
            std::f32::consts::FRAC_PI_2,
        )
    } else {
        let bisector = sum / sum_len_sq.sqrt();
        let cos_full = convex_prev.dot(convex_next).clamp(-1.0, 1.0);
        (bisector, cos_full.acos() * 0.5)
    };
    let n = round_segments(geo.inner_offset);
    push_round_fan(
        center,
        bisector,
        half_angle,
        n,
        geo,
        inner_color,
        call_start_verts,
        out_verts,
        out_indices,
    );
}

/// Emit an arc fan centered at `center`, opening toward
/// `center_dir`, sweeping `±half_angle`. Pushes 1 center vert +
/// 2·(`segments`+1) arc verts (alternating inner / outer fringe).
#[allow(clippy::too_many_arguments)]
fn push_round_fan(
    center: Vec2,
    center_dir: Vec2,
    half_angle: f32,
    segments: u16,
    geo: Geo,
    inner_color: Color,
    call_start_verts: usize,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = segments.max(1);
    let step = 2.0 * half_angle / n as f32;
    let start_angle = -half_angle;
    let perp = Vec2::new(-center_dir.y, center_dir.x);
    let base = current_offset(out_verts.len(), call_start_verts);
    out_verts.push(MeshVertex {
        pos: center,
        color: inner_color,
    });
    for k in 0..=n {
        let angle = start_angle + k as f32 * step;
        let (s, c) = angle.sin_cos();
        let dir = c * center_dir + s * perp;
        out_verts.push(MeshVertex {
            pos: center + dir * geo.inner_offset,
            color: inner_color,
        });
        out_verts.push(MeshVertex {
            pos: center + dir * geo.outer_offset,
            color: Color::TRANSPARENT,
        });
    }
    for k in 0..n {
        let inner_k = base + 1 + 2 * k;
        let outer_k = base + 2 + 2 * k;
        let inner_k1 = base + 1 + 2 * (k + 1);
        let outer_k1 = base + 2 + 2 * (k + 1);
        out_indices.extend_from_slice(&[base, inner_k, inner_k1]);
        out_indices.extend_from_slice(&[inner_k, outer_k, outer_k1]);
        out_indices.extend_from_slice(&[inner_k, outer_k1, inner_k1]);
    }
}

#[inline]
fn push_cross_section(
    p: Vec2,
    normal: Vec2,
    ext: f32,
    geo: Geo,
    inner_color: Color,
    out_verts: &mut Vec<MeshVertex>,
) {
    let outer = normal * (geo.outer_offset * ext);
    let inner = normal * (geo.inner_offset * ext);
    out_verts.push(MeshVertex {
        pos: p + outer,
        color: Color::TRANSPARENT,
    });
    out_verts.push(MeshVertex {
        pos: p + inner,
        color: inner_color,
    });
    out_verts.push(MeshVertex {
        pos: p - inner,
        color: inner_color,
    });
    out_verts.push(MeshVertex {
        pos: p - outer,
        color: Color::TRANSPARENT,
    });
}

/// Three quads per segment: outer-left fringe, full-α core,
/// outer-right fringe. `a` and `b` are u16 vert offsets to the two
/// cross-section blocks bracketing the segment.
#[inline]
fn push_strip_indices(a: u16, b: u16, out: &mut Vec<u16>) {
    out.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
    out.extend_from_slice(&[a + 1, a + 2, b + 2, a + 1, b + 2, b + 1]);
    out.extend_from_slice(&[a + 2, a + 3, b + 3, a + 2, b + 3, b + 2]);
}

#[inline]
fn scale_alpha(c: Color, s: f32) -> Color {
    Color {
        r: c.r * s,
        g: c.g * s,
        b: c.b * s,
        a: c.a * s,
    }
}

#[inline]
fn avg_color(x: Color, y: Color) -> Color {
    Color {
        r: (x.r + y.r) * 0.5,
        g: (x.g + y.g) * 0.5,
        b: (x.b + y.b) * 0.5,
        a: (x.a + y.a) * 0.5,
    }
}

/// Segment tangent given its normal. `normal = (-dy, dx)` ⇒
/// `tangent = (dx, dy) = (normal.y, -normal.x)`.
#[inline]
fn tangent_of(normal: Vec2) -> Vec2 {
    Vec2::new(normal.y, -normal.x)
}

#[inline]
fn seg_normal(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let len_sq = d.length_squared();
    if len_sq < 1e-12 {
        return Vec2::Y;
    }
    let d = d / len_sq.sqrt();
    Vec2::new(-d.y, d.x)
}

/// Current per-call vertex offset as u16. Panics with a clear
/// message if the polyline's emitted vert count overflows the u16
/// index space — composer's `base_vertex` shifts these later, but
/// indices themselves are u16.
#[inline]
fn current_offset(verts_len: usize, call_start: usize) -> u16 {
    u16::try_from(verts_len - call_start).expect("polyline tessellation exceeded u16 vertex limit")
}

#[cfg(test)]
mod tests;
