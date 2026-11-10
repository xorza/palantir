//! What a pipeline that draws the shape template ([`wgsl::SHAPE_TEMPLATE`]) needs beside its
//! text: the constant it substitutes, the vertices it draws, and the attributes of the
//! [`ShapeHeader`] its instance opens with.

use crate::gpu::pipeline::pipeline_recipe::vertex_attr;
use crate::gpu::pipeline::shader_body::ShaderConstant;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::wgsl;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use glam::Vec2;
use std::mem::offset_of;

/// The number [`wgsl::SHAPE_TEMPLATE`] takes, which each includer lists among its constants.
pub(crate) const CONSTANT: ShaderConstant =
    ShaderConstant::uint("ARC_SEGMENTS", wgsl::ARC_SEGMENTS);

/// The vertices `form` draws each instance with: a rect's strip of four, or the template of its
/// polygon's corners, each corner's ring, each edge's band, and the interior fan over every
/// corner's inner chain (`palantir_template_vertex`).
pub(crate) const fn vertices(form: ShapeForm) -> u32 {
    let corner_count = match form {
        ShapeForm::Rect => return 4,
        ShapeForm::Box => 4,
        ShapeForm::Triangle => 3,
    };
    let rings = corner_count * (2 * wgsl::ARC_SEGMENTS + 1);
    let bands = corner_count * 4;
    let interior = corner_count * (wgsl::ARC_SEGMENTS + 1) - 2;
    3 * (rings + bands + interior)
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
    use crate::gpu::pipeline::shape_template;
    use crate::renderer::render_buffer::shape_form::ShapeForm;

    /// A rect is a strip of four; a box's template is 4 rings of 5 triangles, 4 bands of 4 and an
    /// interior fan of 10; a triangle's 3 rings, 3 bands and a fan of 7.
    #[test]
    fn each_form_draws_its_shapes_vertices() {
        assert_eq!(shape_template::vertices(ShapeForm::Rect), 4);
        assert_eq!(shape_template::vertices(ShapeForm::Box), 3 * (20 + 16 + 10));
        assert_eq!(
            shape_template::vertices(ShapeForm::Triangle),
            3 * (15 + 12 + 7)
        );
    }
}
