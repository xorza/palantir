//! What a pipeline that draws the shape template ([`wgsl::SHAPE_TEMPLATE`]) needs beside its
//! text: the constant it substitutes, the indices it draws, and the attributes of the
//! [`ShapeHeader`] its instance opens with.

use crate::gpu::device::gpu::{FilledBuffer, Gpu};
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::pipeline_recipe::vertex_attr;
use crate::gpu::pipeline::shader_body::ShaderConstant;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::wgsl;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use glam::Vec2;
use std::mem::offset_of;
use std::ops::Range;

/// The number [`wgsl::SHAPE_TEMPLATE`] takes, which each includer lists among its constants.
pub(crate) const CONSTANT: ShaderConstant =
    ShaderConstant::uint("ARC_SEGMENTS", wgsl::ARC_SEGMENTS);

// `palantir_template_point` writes the arc's directions as closed forms for two segments.
const _: () = assert!(wgsl::ARC_SEGMENTS == 2);

/// The triangles of the template of `n` corners: each corner's ring, each edge's band, and the
/// interior fan over every corner's inner chain.
const fn triangle_count(n: u32) -> u32 {
    let rings = n * (2 * wgsl::ARC_SEGMENTS + 1);
    let bands = n * 4;
    let interior = n * (wgsl::ARC_SEGMENTS + 1) - 2;
    rings + bands + interior
}

/// The points of the template of `n` corners, each one vertex: per corner its ring's outer and
/// inner chains, per edge its band's hexagon, then the interior fan's points
/// (`palantir_template_vertex`).
const fn point_count(n: u32) -> u32 {
    n * (2 * wgsl::ARC_SEGMENTS + 3) + n * 6 + n * (wgsl::ARC_SEGMENTS + 1)
}

/// The point the `at`th vertex of triangle `tri` of the template of `n` corners is. Each ring is
/// a strip between its chains, `(o_s, o_s+1, q_s)` on even steps and `(q_s, o_s+1, q_s+1)` on odd
/// ones; each band 4 triangles over its hexagon, 3 nibbles of hexagon points each; the interior a
/// fan from the first corner's first inner point.
const fn point_of(n: u32, tri: u32, at: u32) -> u32 {
    const BAND_HEXAGON: [u32; 4] = [0x210, 0x520, 0x325, 0x435];
    let a = wgsl::ARC_SEGMENTS;
    let ring_triangles = 2 * a + 1;
    let ring_points = 2 * a + 3;
    let rings = n * ring_triangles;
    let bands = n * 4;
    if tri < rings {
        let t = tri % ring_triangles;
        let odd = t % 2 == 1;
        let inner = (odd && at != 1) || (!odd && at == 2);
        let j = t / 2 + (at == 1 || (odd && at == 2)) as u32;
        (tri / ring_triangles) * ring_points + if inner { a + 2 + j } else { j }
    } else if tri < rings + bands {
        let band = tri - rings;
        let hexagon = (BAND_HEXAGON[(band % 4) as usize] >> (4 * at)) & 0xf;
        n * ring_points + (band / 4) * 6 + hexagon
    } else {
        let fan = if at > 0 { tri - rings - bands + at } else { 0 };
        n * ring_points + n * 6 + fan
    }
}

/// The template of `n` corners as `N` indices of its points, three a triangle.
const fn indices<const N: usize>(n: u32) -> [u16; N] {
    assert!(N == 3 * triangle_count(n) as usize);
    assert!(point_count(n) <= u16::MAX as u32);
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = point_of(n, i as u32 / 3, i as u32 % 3) as u16;
        i += 1;
    }
    out
}

const BOX_INDEX_COUNT: usize = 3 * triangle_count(4) as usize;
const TRIANGLE_INDEX_COUNT: usize = 3 * triangle_count(3) as usize;

/// A box's template, then a triangle's, as one index buffer holds them.
const INDICES: ([u16; BOX_INDEX_COUNT], [u16; TRIANGLE_INDEX_COUNT]) = (indices(4), indices(3));

/// The template's index buffer, which a pipeline that draws closed shapes binds: each point is one
/// vertex, which the triangles that share it reuse, as a quad's corner is in a strip. The vertex
/// stage computes a point from its index alone (`palantir_template_vertex`). Refused: a vertex
/// per triangle corner, 138 a box where 64 points serve, which on V3D ran its vertex stage, about
/// 500 instructions, 2.2 times as often (`.notes/PERF.md`, "Shape templates indexed").
#[derive(Debug)]
pub(crate) struct TemplateIndices {
    buffer: wgpu::Buffer,
}

impl TemplateIndices {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let parts: [&[u8]; 2] = [
            bytemuck::cast_slice(&INDICES.0),
            bytemuck::cast_slice(&INDICES.1),
        ];
        let len = size_of_val(&INDICES.0) + size_of_val(&INDICES.1);
        Self {
            buffer: gpu.create_filled_buffer(
                FilledBuffer {
                    label: "palantir.template.indices",
                    size: (len as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
                    usage: wgpu::BufferUsages::INDEX,
                },
                &parts,
            ),
        }
    }

    /// Draw `instances` of `form`: a rect's strip of four, or its template, indexed.
    pub(crate) fn draw<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        form: ShapeForm,
        instances: Range<u32>,
    ) {
        match form {
            ShapeForm::Rect => rec.draw(0..4, instances),
            ShapeForm::Box | ShapeForm::Triangle => {
                rec.index(&self.buffer, wgpu::IndexFormat::Uint16);
                rec.draw_indexed(Self::range(form), 0, instances);
            }
        }
    }

    /// Where `form`'s template lies in the buffer.
    const fn range(form: ShapeForm) -> Range<u32> {
        let box_end = BOX_INDEX_COUNT as u32;
        match form {
            ShapeForm::Box => 0..box_end,
            ShapeForm::Triangle => box_end..box_end + TRIANGLE_INDEX_COUNT as u32,
            ShapeForm::Rect => unreachable!(),
        }
    }
}

/// The attributes of the [`ShapeHeader`] an instance opens with, at offset 0, as
/// `PalantirShapeIn` (`shape_template.wgsl`) reads them at locations 0 to 5; a family's own fields
/// follow from [`HEADER_LOCATIONS`]. The rect is one `(min, size)` lane, and the inradius and the
/// turn another.
pub(crate) const HEADER_ATTRS: [wgpu::VertexAttribute; 6] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Float32x4,
        offset_of!(ShapeHeader, rect),
    ),
    vertex_attr(
        1,
        wgpu::VertexFormat::Float32x4,
        offset_of!(ShapeHeader, points),
    ),
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32x4,
        offset_of!(ShapeHeader, points) + 2 * size_of::<Vec2>(),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Float16x4,
        offset_of!(ShapeHeader, corners),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Float32x2,
        offset_of!(ShapeHeader, inradius),
    ),
    vertex_attr(5, wgpu::VertexFormat::Uint32, offset_of!(ShapeHeader, clip)),
];

/// The first location past the header's, where a family's own fields start.
pub(crate) const HEADER_LOCATIONS: u32 = HEADER_ATTRS.len() as u32;

// The lanes `HEADER_ATTRS` reads two fields through lie side by side.
const _: () = {
    assert!(offset_of!(ShapeHeader, rect) == 0);
    assert!(size_of::<Rect>() == 4 * size_of::<f32>());
    assert!(
        offset_of!(ShapeHeader, rotation) == offset_of!(ShapeHeader, inradius) + size_of::<f32>()
    );
};

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::shader_body::ShaderBody;
    use crate::gpu::pipeline::shape_template::{self, INDICES, point_count, triangle_count};
    use crate::primitives::wgsl::SHAPE_TEMPLATE;
    use std::collections::BTreeSet;

    /// A box's template is 4 rings of 5 triangles, 4 bands of 4 and an interior fan of 10, over
    /// 64 points: per corner a ring's 4 outer and 3 inner, per edge a hexagon, and 3 inner points
    /// a corner for the fan. A triangle's 3 rings, 3 bands and a fan of 7, over 21 + 18 + 9.
    #[test]
    fn each_form_draws_its_template_over_its_points() {
        assert_eq!(triangle_count(4), 20 + 16 + 10);
        assert_eq!(point_count(4), 28 + 24 + 12);
        assert_eq!(triangle_count(3), 15 + 12 + 7);
        assert_eq!(point_count(3), 21 + 18 + 9);
    }

    /// Each triangle names the points `palantir_template_vertex` lays out, every point once at
    /// least: a box's first ring triangle `(o_0, o_1, q_0)` is points 0, 1 and 4 (the inner
    /// chain follows the 4 outer points), its second `(q_0, o_1, q_1)` 4, 1 and 5; its first band
    /// triangle the hexagon's `O_e, O_next, I_next`, past the 28 ring points; its interior fan from
    /// point 52, past the 24 band points, the last triangle closing on 62 and 63.
    #[test]
    fn the_indices_name_every_point_of_the_template() {
        let (boxes, triangles) = (&INDICES.0, &INDICES.1);
        assert_eq!(boxes[..6], [0, 1, 4, 4, 1, 5]);
        assert_eq!(boxes[3 * 20..3 * 20 + 3], [28, 29, 30]);
        assert_eq!(boxes[3 * 36..3 * 36 + 3], [52, 53, 54]);
        assert_eq!(boxes[3 * 45..], [52, 62, 63]);
        for (indices, n) in [(&boxes[..], 4), (&triangles[..], 3)] {
            let named: BTreeSet<u16> = indices.iter().copied().collect();
            assert_eq!(named, (0..point_count(n) as u16).collect());
        }
    }

    /// The template before it drew indexed: a vertex per triangle corner, its arc's directions by
    /// `acos` and `sin`. Kept as the reference the indexed template is held to.
    const BEFORE: &str = "
fn before_arc_dir(na: vec2<f32>, nb: vec2<f32>, f: f32) -> vec2<f32> {
    if (f <= 0.0) { return na; }
    if (f >= 1.0) { return nb; }
    let sweep = acos(clamp(dot(na, nb), -1.0, 1.0));
    return (sin((1.0 - f) * sweep) * na + sin(f * sweep) * nb) / sin(sweep);
}

fn before_point(k: PalantirCorner, inradius: f32, kind: u32, j: u32, depth: f32) -> vec2<f32> {
    let segments = f32(PALANTIR_ARC_SEGMENTS);
    let m = (k.na + k.nb) / (1.0 + dot(k.na, k.nb));
    if (kind == PALANTIR_POINT_DEEP) { return k.c + max(k.r - depth, k.r - inradius) * m; }
    if (kind == PALANTIR_POINT_INNER) {
        return k.c + max(k.r - depth, 0.0) * before_arc_dir(k.na, k.nb, f32(j) / segments);
    }
    let reach = k.r + PALANTIR_AA_HALF_WIDTH;
    if (j == 0u) { return k.c + reach * k.na; }
    if (j == PALANTIR_ARC_SEGMENTS + 1u) { return k.c + reach * k.nb; }
    let half_step = 0.5 * acos(clamp(dot(k.na, k.nb), -1.0, 1.0)) / segments;
    return k.c + (reach / cos(half_step)) * before_arc_dir(k.na, k.nb, (f32(j) - 0.5) / segments);
}

const BEFORE_HEXAGON = vec4<u32>(0x210u, 0x520u, 0x325u, 0x435u);

fn before_vertex(s: PalantirShape, vi: u32, border_width: f32) -> PalantirTemplateVertex {
    let depth = border_width + PALANTIR_AA_HALF_WIDTH;
    let ring_triangles = 2u * PALANTIR_ARC_SEGMENTS + 1u;
    let tri = vi / 3u;
    let at = vi % 3u;
    let rings = s.n * ring_triangles;
    let bands = s.n * 4u;
    var out: PalantirTemplateVertex;
    var kind = PALANTIR_POINT_OUTER;
    var i = 0u;
    var j = 0u;
    var edge = 0u;
    if (tri < rings) {
        out.region = PALANTIR_REGION_RING;
        i = tri / ring_triangles;
        let t = tri % ring_triangles;
        let odd = t % 2u == 1u;
        let inner = (odd && at != 1u) || (!odd && at == 2u);
        kind = select(PALANTIR_POINT_OUTER, PALANTIR_POINT_INNER, inner);
        j = t / 2u + u32(at == 1u || (odd && at == 2u));
    } else if (tri < rings + bands) {
        out.region = PALANTIR_REGION_BAND;
        edge = (tri - rings) / 4u;
        let h = (BEFORE_HEXAGON[(tri - rings) % 4u] >> (4u * at)) & 0xfu;
        let next = (edge + 1u) % s.n;
        let first = h == 1u || h == 2u || h == 3u;
        i = select(edge, next, first);
        if (h <= 1u) {
            kind = PALANTIR_POINT_OUTER;
            j = select(PALANTIR_ARC_SEGMENTS + 1u, 0u, first);
        } else if (h == 2u || h == 5u || palantir_shape_corner(s, i).r >= depth) {
            kind = PALANTIR_POINT_INNER;
            j = select(PALANTIR_ARC_SEGMENTS, 0u, first);
        } else {
            kind = PALANTIR_POINT_DEEP;
        }
    } else {
        out.region = PALANTIR_REGION_INTERIOR;
        let fan = select(0u, tri - rings - bands + at, at > 0u);
        i = fan / (PALANTIR_ARC_SEGMENTS + 1u);
        j = fan % (PALANTIR_ARC_SEGMENTS + 1u);
        kind = select(PALANTIR_POINT_DEEP, PALANTIR_POINT_INNER, palantir_shape_corner(s, i).r >= depth);
    }
    let k = palantir_shape_corner(s, i);
    out.pos = before_point(k, s.inradius, kind, j, depth);
    out.ring_r = k.r;
    if (out.region == PALANTIR_REGION_RING) {
        out.geo = out.pos - k.c;
    } else if (out.region == PALANTIR_REGION_BAND) {
        let e = palantir_shape_corner(s, edge);
        out.geo = vec2<f32>(dot(out.pos - e.c, e.nb) - e.r, 0.0);
    } else {
        out.geo = vec2<f32>(0.0);
    }
    return out;
}

@group(0) @binding(14) var<storage, read> probe_in: array<array<vec4<f32>, 5>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<array<vec4<f32>, 4>>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    var s: PalantirShape;
    s.n = u32(q[3].z);
    s.p0 = q[0].xy;
    s.p1 = q[0].zw;
    s.p2 = q[1].xy;
    s.p3 = q[1].zw;
    s.radius = q[2];
    s.inradius = q[3].x;
    let now = palantir_template_vertex(s, u32(q[4].y), q[3].y);
    let then = before_vertex(s, u32(q[4].x), q[3].y);
    probe_out[id.x] = array<vec4<f32>, 4>(
        vec4<f32>(f32(now.region), now.ring_r, now.pos),
        vec4<f32>(now.geo, 0.0, 0.0),
        vec4<f32>(f32(then.region), then.ring_r, then.pos),
        vec4<f32>(then.geo, 0.0, 0.0),
    );
}
";

    /// Every vertex of the indexed template, on the GPU, against the template before it, a
    /// vertex per triangle corner with its arc's directions by `acos` and `sin`: the same region
    /// and arc radius, bit for bit, and the same point and distance within 2e-3 of the larger of
    /// 1 and the value. The tolerance is the reference's trigonometry: WGSL holds `sin` and `cos`
    /// to 2⁻¹¹ absolute and `acos` looser; an Apple GPU's lands within 2e-5 of the closed form
    /// and lavapipe's, in CI, 4.7e-4 off it at a triangle's corner. Over a rounded box
    /// with each corner its own radius, at a border its smaller corners are not deep enough for, a
    /// circle, a thin sharp box and a rounded triangle with and without a border.
    #[test]
    fn the_indexed_template_draws_the_triangles_it_drew_before() {
        /// A shape as the template reads it, and the border it draws.
        #[derive(Clone, Copy, Debug)]
        struct Shape {
            points: [f32; 8],
            radius: [f32; 4],
            inradius: f32,
            border: f32,
            corners: u32,
        }
        let rect = |w: f32, h: f32, radius: [f32; 4], border: f32| Shape {
            points: [0.0, 0.0, w, 0.0, w, h, 0.0, h],
            radius,
            inradius: w.min(h) / 2.0,
            border,
            corners: 4,
        };
        // Inradius `2 · area / perimeter`: 8000 over 100 + 106.3 + 85.44.
        let triangle = |border: f32| Shape {
            points: [0.0, 0.0, 100.0, 0.0, 30.0, 80.0, 0.0, 0.0],
            radius: [6.0, 3.0, 10.0, 0.0],
            inradius: 27.42,
            border,
            corners: 3,
        };
        let shapes = [
            rect(120.0, 80.0, [12.0, 0.0, 30.0, 4.0], 0.0),
            rect(120.0, 80.0, [12.0, 0.0, 30.0, 4.0], 6.0),
            rect(50.0, 50.0, [25.0; 4], 2.0),
            rect(200.0, 10.0, [0.0; 4], 1.0),
            triangle(0.0),
            triangle(4.0),
        ];
        let mut inputs: Vec<[[f32; 4]; 5]> = Vec::new();
        for shape in shapes {
            let p = shape.points;
            let indices: &[u16] = if shape.corners == 4 {
                &INDICES.0
            } else {
                &INDICES.1
            };
            for (vi, &u) in indices.iter().enumerate() {
                inputs.push([
                    [p[0], p[1], p[2], p[3]],
                    [p[4], p[5], p[6], p[7]],
                    shape.radius,
                    [shape.inradius, shape.border, shape.corners as f32, 0.0],
                    [vi as f32, f32::from(u), 0.0, 0.0],
                ]);
            }
        }
        let shader = ShaderBody {
            label: "palantir.template",
            wgsl: BEFORE,
            includes: &[SHAPE_TEMPLATE],
            constants: &[shape_template::CONSTANT],
        };
        let got: Vec<[[f32; 4]; 4]> = shader.probe("", &inputs);
        for (input, [now, now_geo, then, then_geo]) in inputs.iter().zip(got) {
            let at = (input[3], input[4][0]);
            assert_eq!(now[..2], then[..2], "{at:?}: region and arc radius");
            for (g, w) in now[2..]
                .iter()
                .chain(&now_geo[..2])
                .zip(then[2..].iter().chain(&then_geo[..2]))
            {
                assert!(
                    (g - w).abs() <= 2e-3 * w.abs().max(1.0),
                    "{at:?}: {now:?} {now_geo:?} against {then:?} {then_geo:?}",
                );
            }
        }
    }
}
