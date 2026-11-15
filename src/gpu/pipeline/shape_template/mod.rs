//! What a pipeline that draws the shape template ([`wgsl::SHAPE_TEMPLATE`]) needs beside its
//! text: the constant it substitutes, the indices it draws, and the attributes of the
//! [`ShapeHeader`] its instance opens with.

use crate::gpu::device::gpu::{FilledBuffer, Gpu};
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::resource::pipeline_recipe::vertex_attr;
use crate::gpu::resource::shader_body::ShaderConstant;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::wgsl;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use crate::renderer::render_buffer::template_cover::TemplateCover;
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
    rings + bands + interior_count(n)
}

/// The interior fan's triangles of the template of `n` corners, which its index buffer lists last.
const fn interior_count(n: u32) -> u32 {
    n * (wgsl::ARC_SEGMENTS + 1) - 2
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
#[derive(Debug)]
struct Indices {
    boxes: [u16; BOX_INDEX_COUNT],
    triangles: [u16; TRIANGLE_INDEX_COUNT],
}

const INDICES: Indices = Indices {
    boxes: indices(4),
    triangles: indices(3),
};

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
            bytemuck::cast_slice(&INDICES.boxes),
            bytemuck::cast_slice(&INDICES.triangles),
        ];
        let len = size_of_val(&INDICES.boxes) + size_of_val(&INDICES.triangles);
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

    /// Draw `instances` of `form`, `cover` of each: a rect's strip of four, or its template,
    /// indexed.
    pub(crate) fn draw<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        form: ShapeForm,
        cover: TemplateCover,
        instances: Range<u32>,
    ) {
        match form {
            ShapeForm::Rect => {
                debug_assert_eq!(cover, TemplateCover::Whole, "a rect's strip has no rim");
                rec.draw(0..4, instances);
            }
            ShapeForm::Box | ShapeForm::Triangle => {
                rec.index(&self.buffer, wgpu::IndexFormat::Uint16);
                rec.draw_indexed(Self::range(form, cover), 0, instances);
            }
        }
    }

    /// Where `cover` of `form`'s template lies in the buffer: the interior's triangles last, so a
    /// rim is the whole less them.
    const fn range(form: ShapeForm, cover: TemplateCover) -> Range<u32> {
        let (start, n) = match form {
            ShapeForm::Box => (0, 4),
            ShapeForm::Triangle => (BOX_INDEX_COUNT as u32, 3),
            ShapeForm::Rect => unreachable!(),
        };
        let triangles = match cover {
            TemplateCover::Whole => triangle_count(n),
            TemplateCover::Rim => triangle_count(n) - interior_count(n),
        };
        start..start + 3 * triangles
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
mod tests;
