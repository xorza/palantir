use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::shape::{ColorMode, LineCap, LineJoin};
use glam::Vec2;

const HALF_FRINGE: f32 = 0.5;
/// SVG default. Beyond this the join would project a long spike;
/// clamp the miter factor here and accept the cut-off corner. v1
/// stays clamp-only; proper bevel for curve work can land next to
/// the curve-flattening change.
const MITER_LIMIT: f32 = 4.0;

/// Tessellate a stroked polyline as a fringe-AA mesh.
///
/// Inputs are in **physical px** — composer applies the active
/// transform + DPI scale to `points` and `width_phys` before
/// calling. Colors are premultiplied linear RGBA. `mode` picks
/// the color-storage interpretation and the vertex layout:
///
/// - [`ColorMode::Single`] — `colors.len() == 1`. Same color on
///   every cross-section. 4 verts per input point.
/// - [`ColorMode::PerPoint`] — `colors.len() == points.len()`. GPU
///   lerps between adjacent cross-sections, giving a smooth
///   gradient along the stroke. 4 verts per input point.
/// - [`ColorMode::PerSegment`] — `colors.len() == points.len() - 1`.
///   Each segment paints as a solid block; interior cross-sections
///   duplicate so colors don't bleed at joins. 4 verts per
///   endpoint plus 8 verts per interior point — total `8N - 8` for
///   `N >= 2`.
///
/// **Hairline behavior.** For `width_phys < 1`, geometry freezes
/// at 1 physical px wide and per-vertex colors are alpha-scaled by
/// `width_phys` (premultiplied → rgb and alpha by the same
/// factor). A 0.3-px line paints as a 1-px line at α=0.3 of each
/// vertex's input color. Layout stays branchless via
/// `half_geom = max(w/2, 0.5)` so vertex count is identical at
/// every width within a given mode.
///
/// **Joins.** Miter with factor clamped to [`MITER_LIMIT`].
/// **Caps.** Butt (no end extension).
///
/// **Indexing.** Indices are pushed **0-based** to the verts this
/// call emits — composer captures `phys_v_start = out_verts.len()`
/// before calling and passes it as `MeshDraw.vertices.start`,
/// which becomes the wgpu `base_vertex`. Multiple calls into the
/// same vecs concatenate independent index blocks.
/// Stroke configuration bundle. Keeps [`tessellate_polyline_aa`]'s
/// signature at 5 args (vs 8) — the four style/mode params travel
/// together from `DrawPolylinePayload` decode in the composer
/// straight into the tessellator.
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
    let StrokeStyle {
        mode,
        cap,
        join,
        width_phys,
    } = style;
    debug_assert!(matches_mode(points.len(), colors.len(), mode));

    let half_geom = (width_phys * 0.5).max(HALF_FRINGE);
    let alpha_scale = width_phys.clamp(0.0, 1.0);
    let outer_offset = half_geom + HALF_FRINGE;
    let inner_offset = half_geom;

    let n = points.len();
    // Worst-case vert count across all modes: 8 per point (Single
    // + PerPoint with every interior beveled, or PerSegment which
    // always doubles interior points). Front-loaded check so
    // emit_per_segment can use straight arithmetic; emit_simple
    // also has a per-push `checked_add` for defense in depth.
    assert!(
        8 * n <= u16::MAX as usize,
        "polyline too long for u16 indices ({n} points)"
    );

    let geo = Geo {
        outer_offset,
        inner_offset,
        alpha_scale,
        cap_extension: match cap {
            LineCap::Butt => 0.0,
            LineCap::Square => inner_offset,
        },
        force_bevel: matches!(join, LineJoin::Bevel),
    };
    match mode {
        ColorMode::Single | ColorMode::PerPoint => {
            emit_simple(points, colors, mode, geo, out_verts, out_indices);
        }
        ColorMode::PerSegment => {
            emit_per_segment(points, colors, geo, out_verts, out_indices);
        }
    }
}

/// Geometry + style parameters shared by both emit paths. Pre-
/// computed in [`tessellate_polyline_aa`]'s setup so the inner
/// loops just read the resolved values. Includes the `cap` /
/// `join` enums so the emit functions don't grow argument lists
/// (clippy's too-many-arguments lint is real).
#[derive(Clone, Copy)]
struct Geo {
    outer_offset: f32,
    inner_offset: f32,
    alpha_scale: f32,
    cap_extension: f32,
    force_bevel: bool,
}

/// Length check matching [`tessellate_polyline_aa`]'s contract.
fn matches_mode(points_len: usize, colors_len: usize, mode: ColorMode) -> bool {
    match mode {
        ColorMode::Single => colors_len == 1,
        ColorMode::PerPoint => colors_len == points_len,
        ColorMode::PerSegment => colors_len + 1 == points_len,
    }
}

#[inline]
fn pick_color(colors: &[Color], i: usize, mode: ColorMode) -> Color {
    match mode {
        ColorMode::Single => colors[0],
        ColorMode::PerPoint => colors[i],
        // Caller is responsible for emit_per_segment branching;
        // this never sees PerSegment.
        ColorMode::PerSegment => unreachable!(),
    }
}

/// Single + PerPoint emission: one cross-section per input point
/// for non-sharp joins; two cross-sections (a bevel) when the miter
/// factor would exceed [`MITER_LIMIT`]. A miter-clamp produces a
/// visible cut-off at very sharp angles; the bevel cleanly fills
/// the corner with a bridging quad on the convex side.
///
/// Vertex layout per interior point: 4 verts if mitered, 8 if
/// beveled. Endpoints are always 4. Total = `4*N + 4*B` where `B`
/// is the number of beveled joins. Strip indexing tracks the
/// per-point block offset via the `point_offsets` table built in
/// pass 1.
fn emit_simple(
    points: &[Vec2],
    colors: &[Color],
    mode: ColorMode,
    geo: Geo,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = points.len();
    // `geo.force_bevel` forces every interior point to bevel
    // (LineJoin::Bevel); Miter mode still falls back at sharp
    // angles via `is_sharp_join`. `geo.cap_extension` is 0 for
    // Butt caps, `half_geom` for Square — geometry-only, no
    // forward fringe (accepted aliasing tradeoff at the cap edge).
    let mut cursor: u16 = 0;
    let mut prev_offset: u16 = 0;
    let mut prev_was_bevel = false;
    let mut prev_seg_normal: Option<Vec2> = None;

    for i in 0..n {
        let next_seg_normal = if i + 1 < n {
            Some(seg_normal(points[i], points[i + 1]))
        } else {
            None
        };
        let is_bevel = match (prev_seg_normal, next_seg_normal) {
            (Some(np), Some(nn)) => geo.force_bevel || is_sharp_join(np, nn),
            _ => false,
        };

        let current_offset = cursor;
        let color = scale_alpha(pick_color(colors, i, mode), geo.alpha_scale);

        // Verts at point `i`.
        match (prev_seg_normal, next_seg_normal) {
            (Some(np), Some(nn)) if is_bevel => {
                push_cross_section(points[i], np, 1.0, geo, color, out_verts);
                push_cross_section(points[i], nn, 1.0, geo, color, out_verts);
            }
            (Some(np), Some(nn)) => {
                let (normal, ext) = miter_bisector(np, nn);
                push_cross_section(points[i], normal, ext, geo, color, out_verts);
            }
            (None, Some(nn)) => {
                // Start endpoint: extend backward along segment 0.
                let p = points[i] - forward_of(nn) * geo.cap_extension;
                push_cross_section(p, nn, 1.0, geo, color, out_verts);
            }
            (Some(np), None) => {
                // End endpoint: extend forward past the last segment.
                let p = points[i] + forward_of(np) * geo.cap_extension;
                push_cross_section(p, np, 1.0, geo, color, out_verts);
            }
            (None, None) => unreachable!("polyline length < 2 short-circuits earlier"),
        }
        cursor = cursor
            .checked_add(if is_bevel { 8 } else { 4 })
            .expect("polyline too long for u16 indices — bevel vertex count exceeded 65535");

        // Bevel bridge at this point, if beveled. References the
        // two cross-section blocks we just pushed.
        if is_bevel {
            let np = prev_seg_normal.unwrap();
            let nn = next_seg_normal.unwrap();
            push_bevel_bridge(current_offset, current_offset + 4, np, nn, out_indices);
        }

        // Strip for segment `(i-1, i)`: leading block at point i-1
        // is `prev_offset + 4` if i-1 was beveled (use the leading
        // duplicate), else `prev_offset`. Trailing block at point
        // i is always `current_offset` — the first block at i.
        if i > 0 {
            let leading = prev_offset + if prev_was_bevel { 4 } else { 0 };
            push_strip_indices(leading, current_offset, out_indices);
        }

        prev_offset = current_offset;
        prev_was_bevel = is_bevel;
        prev_seg_normal = next_seg_normal;
    }
}

/// True iff the miter factor at this join would exceed
/// [`MITER_LIMIT`] — i.e. the inverse cosine of the half-angle
/// breaks the limit. Antiparallel segments (`cos_half ≈ 0`) count
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

/// Bisector direction + miter extension factor (unclamped). Caller
/// must have already determined this join is *not* sharp via
/// [`is_sharp_join`] — otherwise the returned ext could be
/// arbitrarily large.
#[inline]
fn miter_bisector(normal_prev: Vec2, normal_next: Vec2) -> (Vec2, f32) {
    let sum = normal_prev + normal_next;
    let bisector = sum / sum.length();
    let cos_half = bisector.dot(normal_prev);
    (bisector, 1.0 / cos_half)
}

/// Bridge the convex-side gap at a beveled join. `trailing_block`
/// closes the incoming segment (normal `normal_prev`);
/// `leading_block` opens the outgoing segment (normal
/// `normal_next`). The cross product of the two normals picks the
/// convex side: positive → CCW turn → convex on `-normal` side
/// (verts 2,3 in the cross-section); negative → CW turn → convex
/// on `+normal` side (verts 0,1). Emits one quad (2 tris) joining
/// the inner-edge + outer-fringe verts on that side. Mesh pipeline
/// doesn't cull, so winding is informational only.
fn push_bevel_bridge(
    trailing_block: u16,
    leading_block: u16,
    normal_prev: Vec2,
    normal_next: Vec2,
    out: &mut Vec<u16>,
) {
    let cross = normal_prev.perp_dot(normal_next);
    let (inner_off, outer_off) = if cross > 0.0 { (2, 3) } else { (1, 0) };
    let t_inner = trailing_block + inner_off;
    let t_outer = trailing_block + outer_off;
    let l_inner = leading_block + inner_off;
    let l_outer = leading_block + outer_off;
    out.extend_from_slice(&[t_inner, t_outer, l_outer, t_inner, l_outer, l_inner]);
}

/// Per-segment paints each segment in a solid block. Interior
/// cross-sections duplicate (one belonging to segment `i-1`, one
/// to segment `i`) so the strip between two cross-sections
/// belongs to a single segment and carries that segment's color
/// uniformly — no GPU lerp across the join.
///
/// Vertex layout for `N` points (`N >= 2`):
/// - Cross-section 0 (endpoint, 4 verts) — color `colors[0]`.
/// - For each interior point `i in 1..N-1`:
///   - "trailing" cross-section closing segment `i-1` (4 verts) — `colors[i-1]`.
///   - "leading"  cross-section opening segment `i`   (4 verts) — `colors[i]`.
/// - Cross-section `N-1` (endpoint, 4 verts) — `colors[N-2]`.
///
/// Both copies at an interior point share the same position and
/// miter direction — only the color differs.
fn emit_per_segment(
    points: &[Vec2],
    colors: &[Color],
    geo: Geo,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = points.len();
    let segments = n - 1;

    // Roll the segment normal forward across the loop so each
    // segment's perpendicular is computed once, not twice. `np`
    // is the segment ENDING at the current point; `nn` (looked up
    // each iteration for the segment AHEAD) becomes the next
    // iteration's `np`.
    let mut np = seg_normal(points[0], points[1]);

    // Start endpoint, cap-shifted backward along segment 0.
    let p0 = points[0] - forward_of(np) * geo.cap_extension;
    push_cross_section(
        p0,
        np,
        1.0,
        geo,
        scale_alpha(colors[0], geo.alpha_scale),
        out_verts,
    );

    for i in 1..n - 1 {
        // PerSegment always doubles interior points for color
        // separation; bevel/miter choice only affects the
        // *position* of those duplicates. Sharp joins (or
        // `LineJoin::Bevel`) emit both with their own segment
        // normal at ext=1; mitered joins share the bisector
        // direction with ext factor.
        let nn = seg_normal(points[i], points[i + 1]);
        let beveled = geo.force_bevel || is_sharp_join(np, nn);
        let (trailing_normal, trailing_ext, leading_normal, leading_ext) = if beveled {
            (np, 1.0, nn, 1.0)
        } else {
            let (b, ext) = miter_bisector(np, nn);
            (b, ext, b, ext)
        };
        push_cross_section(
            points[i],
            trailing_normal,
            trailing_ext,
            geo,
            scale_alpha(colors[i - 1], geo.alpha_scale),
            out_verts,
        );
        push_cross_section(
            points[i],
            leading_normal,
            leading_ext,
            geo,
            scale_alpha(colors[i], geo.alpha_scale),
            out_verts,
        );
        np = nn;
    }

    // End endpoint, cap-shifted forward along the last segment
    // (`np` is now the segment ending at `points[n-1]`).
    let pl = points[n - 1] + forward_of(np) * geo.cap_extension;
    push_cross_section(
        pl,
        np,
        1.0,
        geo,
        scale_alpha(colors[segments - 1], geo.alpha_scale),
        out_verts,
    );

    // Strip indexing: segment `seg` (0-based) connects
    //   "leading"   cross-section at point `seg`   (block index `2*seg`),
    //   "trailing" cross-section at point `seg+1` (block index `2*seg + 1`).
    // Block size = 4 verts. Endpoints have a single block each;
    // interior points contribute two adjacent blocks.
    for seg in 0..segments {
        let a = (2 * seg * 4) as u16;
        let b = ((2 * seg + 1) * 4) as u16;
        push_strip_indices(a, b, out_indices);
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
    let outer_color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    out_verts.push(MeshVertex {
        pos: p + outer,
        color: outer_color,
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
        color: outer_color,
    });
}

/// Three quads per segment: outer-left fringe, full-α core,
/// outer-right fringe. CCW with the left perpendicular as
/// `+normal`. `a` and `b` are u16 vert offsets to the two
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

/// Convert a segment normal back to its forward direction.
/// `normal = (-dy, dx)` ⇒ `forward = (dx, dy) = (normal.y, -normal.x)`.
#[inline]
fn forward_of(normal: Vec2) -> Vec2 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Color {
        Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }

    fn green() -> Color {
        Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        }
    }

    /// Single-color: horizontal 2-point line at width=2.
    /// 8 verts (4 per cross-section), 18 indices. `seg_normal`
    /// returns `(-dy, dx) = (0, +1)` for a +x segment, so
    /// "outer-left" (+normal) sits at y = +1.5.
    #[test]
    fn single_horizontal_line_geometry() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 8);
        assert_eq!(i.len(), 18);
        assert_eq!(v[0].pos, Vec2::new(0.0, 1.5));
        assert_eq!(v[0].color.a, 0.0);
        assert_eq!(v[1].pos, Vec2::new(0.0, 1.0));
        assert_eq!(v[1].color, red());
        assert_eq!(v[2].pos, Vec2::new(0.0, -1.0));
        assert_eq!(v[2].color, red());
        assert_eq!(v[3].pos, Vec2::new(0.0, -1.5));
        assert_eq!(v[3].color.a, 0.0);
    }

    /// Hairline freeze + alpha fade applies per-vertex with input
    /// color preserved (modulo the scale).
    #[test]
    fn hairline_alpha_scales_input_color() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 0.3,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 8);
        // Geometry locked at 1 px.
        assert_eq!(v[0].pos, Vec2::new(0.0, 1.0));
        assert_eq!(v[1].pos, Vec2::new(0.0, 0.5));
        // RGB and alpha both scaled by 0.3 (premultiplied
        // contract).
        let inner = v[1].color;
        assert!((inner.r - 0.3).abs() < 1e-6);
        assert!((inner.a - 0.3).abs() < 1e-6);
    }

    /// PerPoint: distinct colors on each cross-section, no
    /// duplication.
    #[test]
    fn per_point_colors_distinct_per_cross_section() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
            &[red(), green(), red()],
            StrokeStyle {
                mode: ColorMode::PerPoint,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 12);
        assert_eq!(v[1].color, red());
        assert_eq!(v[5].color, green());
        assert_eq!(v[9].color, red());
    }

    /// PerSegment: interior cross-section duplicates; both copies
    /// share position but carry the abutting segments' colors.
    #[test]
    fn per_segment_duplicates_interior_cross_sections() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        // 3 points → 2 segments; interior point gets duplicated.
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
            &[red(), green()],
            StrokeStyle {
                mode: ColorMode::PerSegment,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        // 8N - 8 = 16 verts.
        assert_eq!(v.len(), 16);
        // Endpoint 0: red.
        assert_eq!(v[1].color, red());
        // Interior point trailing (segment 0 ends here) = red,
        // leading (segment 1 starts here) = green; same x.
        assert_eq!(v[5].pos.x, 10.0);
        assert_eq!(v[5].color, red());
        assert_eq!(v[9].pos.x, 10.0);
        assert_eq!(v[9].color, green());
        // Endpoint 2: green.
        assert_eq!(v[13].color, green());
        // Segments: two strips of 18 indices each.
        assert_eq!(i.len(), 36);
    }

    /// PerSegment strip-index correctness — pin that segment 0's
    /// strip references vert blocks 0 and 1 (endpoint + trailing
    /// dup), and segment 1's strip references blocks 2 and 3
    /// (leading dup + endpoint). A naive "block index = point
    /// index" map would conflate the duplicates and bleed colors.
    #[test]
    fn per_segment_strip_indexing_skips_join_blocks() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
            &[red(), green()],
            StrokeStyle {
                mode: ColorMode::PerSegment,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        // First six indices = first quad of segment 0's strip:
        // (a, a+1, b+1, a, b+1, b) with a=0 (block 0), b=4 (block 1).
        assert_eq!(&i[0..6], &[0, 1, 5, 0, 5, 4]);
        // Last six indices = third quad of segment 1's strip:
        // a=8 (block 2), b=12 (block 3).
        let last = i.len() - 6;
        assert_eq!(&i[last..], &[10, 11, 15, 10, 15, 14]);
    }

    /// Non-sharp join (≥ ~29° between segments) miters as before:
    /// 4 verts per cross-section, no bevel bridge. Pin keeps the
    /// bevel detection from triggering on routine 90° corners.
    #[test]
    fn shallow_join_stays_miter() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        // 90° corner: miter factor = sqrt(2) ≈ 1.414, far below limit 4.
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 12); // 4 + 4 + 4
        assert_eq!(i.len(), 36); // 2 strips × 18
    }

    /// Sharp join (chevron, angle ≪ 29°) triggers bevel: interior
    /// point gets two cross-sections (8 verts) + a bridge quad
    /// (6 indices). Total verts = 4 + 8 + 4 = 16; indices = 2
    /// strips × 18 + 1 bridge × 6 = 42.
    #[test]
    fn sharp_join_emits_bevel() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        // Near-180° fold at (10, 0). Half-angle cosine ≈ 0.02.
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.5)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 16);
        assert_eq!(i.len(), 42);
        // Bridge quad (6 indices, single-pass emits it before the
        // strip closing back to the previous point) references
        // only the trailing + leading blocks at the beveled
        // interior point — never the endpoint blocks.
        let bridge = &i[0..6];
        for &idx in bridge {
            assert!(
                (4..12).contains(&idx),
                "bevel bridge index {idx} out of trailing/leading block range"
            );
        }
    }

    /// Antiparallel turn (exact 180°) is also classified sharp —
    /// the antiparallel guard inside `is_sharp_join` short-circuits
    /// to `true` rather than dividing by zero. Geometry: bevel
    /// with both cross-sections at the same point.
    #[test]
    fn antiparallel_turn_is_sharp() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(-5.0, 0.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 16, "antiparallel join must bevel");
    }

    /// Degenerate input (< 2 points) emits nothing.
    #[test]
    fn under_two_points_emits_nothing() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        tessellate_polyline_aa(
            &[Vec2::ZERO],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert!(v.is_empty());
        assert!(i.is_empty());
    }

    /// Indices are 0-based to this call's vert block, even when
    /// the output vecs already contain other data.
    #[test]
    fn indices_are_zero_based_per_call() {
        let mut v = vec![MeshVertex::default(); 5];
        let mut i = vec![99u16; 3];
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: 2.0,
            },
            &mut v,
            &mut i,
        );
        assert_eq!(i[0..3], [99, 99, 99]);
        assert!(i[3..].iter().all(|&idx| idx < 8));
    }
}
