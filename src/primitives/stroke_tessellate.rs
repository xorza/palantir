use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::shape::ColorMode;
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
pub(crate) fn tessellate_polyline_aa(
    points: &[Vec2],
    colors: &[Color],
    mode: ColorMode,
    width_phys: f32,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    if points.len() < 2 {
        return;
    }
    debug_assert!(matches_mode(points.len(), colors.len(), mode));

    let half_geom = (width_phys * 0.5).max(HALF_FRINGE);
    let alpha_scale = width_phys.clamp(0.0, 1.0);
    let outer_offset = half_geom + HALF_FRINGE;
    let inner_offset = half_geom;

    let n = points.len();
    let verts_per_call = match mode {
        ColorMode::Single | ColorMode::PerPoint => n * 4,
        ColorMode::PerSegment => 8 * n - 8,
    };
    assert!(
        verts_per_call <= u16::MAX as usize,
        "polyline too long for u16 indices ({n} points, {verts_per_call} verts)"
    );

    let geo = Geo {
        outer_offset,
        inner_offset,
        alpha_scale,
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

/// Geometry parameters shared by both emit paths. Bundled into a
/// struct so the pass-through signatures stay narrow (clippy's
/// too-many-arguments lint is real; the bundle also documents the
/// invariant that these three values come together from
/// [`tessellate_polyline_aa`]'s setup block).
#[derive(Clone, Copy)]
struct Geo {
    outer_offset: f32,
    inner_offset: f32,
    alpha_scale: f32,
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

/// Single + PerPoint share geometry: one cross-section per input
/// point, four verts each, three quads per segment.
fn emit_simple(
    points: &[Vec2],
    colors: &[Color],
    mode: ColorMode,
    geo: Geo,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    let n = points.len();
    for i in 0..n {
        let (normal, ext) = miter_normal(points, i);
        let color = scale_alpha(pick_color(colors, i, mode), geo.alpha_scale);
        push_cross_section(points[i], normal, ext, geo, color, out_verts);
    }
    for seg in 0..(n - 1) {
        let a = (seg * 4) as u16;
        let b = ((seg + 1) * 4) as u16;
        push_strip_indices(a, b, out_indices);
    }
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

    let (n0, _e0) = miter_normal(points, 0);
    push_cross_section(
        points[0],
        n0,
        1.0,
        geo,
        scale_alpha(colors[0], geo.alpha_scale),
        out_verts,
    );

    for i in 1..n - 1 {
        let (normal, ext) = miter_normal(points, i);
        // Trailing cross-section: closes segment i-1 with its color.
        push_cross_section(
            points[i],
            normal,
            ext,
            geo,
            scale_alpha(colors[i - 1], geo.alpha_scale),
            out_verts,
        );
        // Leading cross-section: opens segment i with its color.
        // Same position + miter — only color differs.
        push_cross_section(
            points[i],
            normal,
            ext,
            geo,
            scale_alpha(colors[i], geo.alpha_scale),
            out_verts,
        );
    }

    let (nl, _el) = miter_normal(points, n - 1);
    push_cross_section(
        points[n - 1],
        nl,
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

/// Per-point miter direction + extension. Endpoints return the
/// adjacent segment's normal with `ext = 1`. Interior points
/// return the bisector normal with `ext = 1/cos(theta/2)`,
/// clamped to [`MITER_LIMIT`]. Antiparallel segments
/// (`cos(theta/2) ≈ 0`) fall back to one side's normal.
fn miter_normal(points: &[Vec2], i: usize) -> (Vec2, f32) {
    let n = points.len();
    let prev = (i > 0).then(|| seg_normal(points[i - 1], points[i]));
    let next = (i + 1 < n).then(|| seg_normal(points[i], points[i + 1]));
    match (prev, next) {
        (Some(a), Some(b)) => {
            let sum = a + b;
            let len_sq = sum.length_squared();
            if len_sq < 1e-6 {
                return (a, 1.0);
            }
            let bisector = sum / len_sq.sqrt();
            let cos_half = bisector.dot(a).max(1.0 / MITER_LIMIT);
            (bisector, 1.0 / cos_half)
        }
        (Some(only), None) | (None, Some(only)) => (only, 1.0),
        (None, None) => unreachable!("polyline length < 2 short-circuits earlier"),
    }
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
            ColorMode::Single,
            2.0,
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
            ColorMode::Single,
            0.3,
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
            ColorMode::PerPoint,
            2.0,
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
            ColorMode::PerSegment,
            2.0,
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
            ColorMode::PerSegment,
            2.0,
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

    /// Degenerate input (< 2 points) emits nothing.
    #[test]
    fn under_two_points_emits_nothing() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(&[], &[red()], ColorMode::Single, 2.0, &mut v, &mut i);
        tessellate_polyline_aa(
            &[Vec2::ZERO],
            &[red()],
            ColorMode::Single,
            2.0,
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
            ColorMode::Single,
            2.0,
            &mut v,
            &mut i,
        );
        assert_eq!(i[0..3], [99, 99, 99]);
        assert!(i[3..].iter().all(|&idx| idx < 8));
    }
}
