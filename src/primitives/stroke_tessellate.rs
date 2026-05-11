use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use glam::Vec2;

const HALF_FRINGE: f32 = 0.5;
/// SVG default. Beyond this the join would project a long spike;
/// clamp the miter factor here and accept the cut-off corner. v1
/// stays clamp-only; proper bevel for curve work can land next to
/// the curve-flattening change.
const MITER_LIMIT: f32 = 4.0;

/// Tessellate a stroked polyline as a fringe-AA mesh.
///
/// All inputs are in **physical px** — the composer applies the
/// active transform and DPI scale to `points` and `width_phys`
/// before calling. `color` is premultiplied linear RGBA.
///
/// Vertex layout per polyline point: 4 verts per cross-section in
/// order `[outer-left, inner-left, inner-right, outer-right]`,
/// where "left" is `+normal`. Outer verts have α=0 (fringe),
/// inner verts carry `color` scaled by the hairline factor.
///
/// **Hairline behavior.** For `width_phys < 1`, geometry freezes
/// at 1 physical px wide and `color` is alpha-scaled by
/// `width_phys` — a 0.3-px line paints as a 1-px line at α=0.3.
/// Layout stays branchless via `half_geom = max(w/2, 0.5)` so the
/// vertex count is identical at every width.
///
/// **Joins.** Miter with factor clamped to [`MITER_LIMIT`] —
/// produces a slight cut-off at very sharp angles. **Caps.**
/// Butt (no end extension). Only Line uses this in v1; the
/// polyline form is ready for flattened curves.
///
/// Indices are pushed **0-based** to the verts this call emits —
/// composer captures `phys_v_start = out_verts.len()` before
/// calling and passes that as `MeshDraw.vertices.start`, which
/// becomes the wgpu `base_vertex`. Multiple calls into the same
/// vecs concatenate independent index blocks.
pub(crate) fn tessellate_polyline_aa(
    points: &[Vec2],
    width_phys: f32,
    color: Color,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    if points.len() < 2 {
        return;
    }

    let half_geom = (width_phys * 0.5).max(HALF_FRINGE);
    let alpha_scale = width_phys.clamp(0.0, 1.0);
    let inner_color = Color {
        r: color.r * alpha_scale,
        g: color.g * alpha_scale,
        b: color.b * alpha_scale,
        a: color.a * alpha_scale,
    };
    let outer_color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    let outer_offset = half_geom + HALF_FRINGE;
    let inner_offset = half_geom;

    let n = points.len();
    assert!(
        n.checked_mul(4).is_some_and(|v| v <= u16::MAX as usize),
        "polyline too long for u16 indices ({n} points)"
    );

    for i in 0..n {
        let (normal, ext) = miter_normal(points, i);
        let p = points[i];
        let outer = normal * (outer_offset * ext);
        let inner = normal * (inner_offset * ext);
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

    // Three quads per segment: outer-left fringe, full-α core,
    // outer-right fringe. Each quad → 2 tris. Winding is CCW
    // when `normal` is the left perpendicular and verts go
    // outer-L → inner-L → inner-R → outer-R top-down.
    for seg in 0..(n - 1) {
        let a = (seg * 4) as u16;
        let b = ((seg + 1) * 4) as u16;
        // outer-left strip
        out_indices.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
        // core strip
        out_indices.extend_from_slice(&[a + 1, a + 2, b + 2, a + 1, b + 2, b + 1]);
        // outer-right strip
        out_indices.extend_from_slice(&[a + 2, a + 3, b + 3, a + 2, b + 3, b + 2]);
    }
}

/// Per-point miter direction + extension. Endpoints return the
/// adjacent segment's normal with `ext = 1`. Interior points
/// return the bisector normal with `ext = 1/cos(theta/2)`,
/// clamped to [`MITER_LIMIT`]. Antiparallel segments
/// (`cos(theta/2) ≈ 0`) fall back to one side's normal — the
/// caller's polyline shouldn't U-turn at width-scale anyway.
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

    /// Horizontal 2-point line at width=2: 8 verts (4 per
    /// cross-section), 18 indices (3 quads × 2 tris × 3). Outer
    /// verts at y=±1.5, inner at y=±1.0; outer α=0, inner α=1.
    #[test]
    fn horizontal_line_geometry() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
            2.0,
            red(),
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 8);
        assert_eq!(i.len(), 18);
        // P0 cross-section. `seg_normal` returns the left
        // perpendicular `(-dy, dx)`, which for a +x segment is
        // (0, +1) in screen coords (y-down). So the "outer-left"
        // (+normal) vert sits at y = +1.5.
        assert_eq!(v[0].pos, Vec2::new(0.0, 1.5));
        assert_eq!(v[0].color.a, 0.0);
        assert_eq!(v[1].pos, Vec2::new(0.0, 1.0));
        assert_eq!(v[1].color.a, 1.0);
        assert_eq!(v[2].pos, Vec2::new(0.0, -1.0));
        assert_eq!(v[2].color.a, 1.0);
        assert_eq!(v[3].pos, Vec2::new(0.0, -1.5));
        assert_eq!(v[3].color.a, 0.0);
        // P1 cross-section.
        assert_eq!(v[4].pos.x, 10.0);
        assert_eq!(v[7].pos.x, 10.0);
        // All 18 indices reference verts in [0, 8).
        assert!(i.iter().all(|&idx| idx < 8));
    }

    /// Width below 1 px: geometry locks at 1 px wide; inner
    /// alpha scales by width. Verifies the unified-layout branch
    /// works at the hairline regime.
    #[test]
    fn hairline_freezes_at_1px_alpha_fades() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            0.3,
            red(),
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 8);
        assert_eq!(v[0].pos, Vec2::new(0.0, 1.0));
        assert_eq!(v[1].pos, Vec2::new(0.0, 0.5));
        assert_eq!(v[2].pos, Vec2::new(0.0, -0.5));
        assert_eq!(v[3].pos, Vec2::new(0.0, -1.0));
        // alpha = 0.3 * 1.0 = 0.3 on inner verts (premultiplied).
        assert!((v[1].color.a - 0.3).abs() < 1e-6);
        assert!((v[2].color.a - 0.3).abs() < 1e-6);
        // Outer fringe stays transparent.
        assert_eq!(v[0].color.a, 0.0);
        assert_eq!(v[3].color.a, 0.0);
    }

    /// Vertex count scales 4× point count; index count scales
    /// 18× segment count. Pin for the curve-flattening path.
    #[test]
    fn polyline_vertex_and_index_count() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[
                Vec2::ZERO,
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(20.0, 10.0),
            ],
            2.0,
            red(),
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 4 * 4);
        assert_eq!(i.len(), 18 * 3);
    }

    /// Indices are 0-based to this call's vert block, even when
    /// the output vecs already contain other data — composer
    /// relies on this for `base_vertex` correctness.
    #[test]
    fn indices_are_zero_based_per_call() {
        let mut v = vec![MeshVertex::default(); 5];
        let mut i = vec![99u16; 3];
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            2.0,
            red(),
            &mut v,
            &mut i,
        );
        // Original prefix preserved.
        assert_eq!(i[0..3], [99, 99, 99]);
        // New block is in [0, 8).
        assert!(i[3..].iter().all(|&idx| idx < 8));
    }

    /// Degenerate input (< 2 points) emits nothing.
    #[test]
    fn under_two_points_emits_nothing() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(&[], 2.0, red(), &mut v, &mut i);
        tessellate_polyline_aa(&[Vec2::ZERO], 2.0, red(), &mut v, &mut i);
        assert!(v.is_empty());
        assert!(i.is_empty());
    }
}
