//! GPU side of native parametric strokes (cubic beziers + circular
//! arcs — see `CurveInstance::kind`). One `draw_indexed` per scissor
//! group covers every `CurveInstance` in the group's `GroupBatch` —
//! an immutable index buffer subdivides each instance into
//! [`SEGMENTS_PER_INSTANCE`](crate::renderer::render_buffer::curve::SEGMENTS_PER_INSTANCE)
//! chords while reusing 34 cross-section vertices across 96 indices;
//! the vertex shader offsets the strip perpendicular to the tangent
//! for stroking + AA.
//!
//! Same stencil-variant pattern as [`MeshPipeline`] /
//! [`ImagePipeline`]: rounded-clip frames use a stencil-test pipeline,
//! plain frames use the unconditional one.
//!
//! [`MeshPipeline`]: crate::renderer::backend::mesh_pipeline::MeshPipeline
//! [`ImagePipeline`]: crate::renderer::backend::image_pipeline::ImagePipeline

use crate::primitives::brush::gradient::Spread;
use crate::primitives::fill_wire::FillKind;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::shader_template::{ShaderConstant, specialize};
use crate::renderer::gradient_atlas::ATLAS_ROWS;
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_ARC, CURVE_KIND_CUBIC, CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER,
    CURVE_KIND_JOIN_ROUND, CURVE_KIND_SEGMENT, CurveInstance, SEGMENTS_PER_INSTANCE,
};
use crate::shape::stroke_bounds::{HALF_FRINGE, MITER_LIMIT};
use crate::shape::style::LineCap;
use wgpu::util::DeviceExt;

const INDICES_PER_INSTANCE: u32 = 6 * SEGMENTS_PER_INSTANCE;
const UNIQUE_VERTICES_PER_INSTANCE: u16 = 2 * (SEGMENTS_PER_INSTANCE as u16 + 1);
const CURVE_INDICES: [u16; INDICES_PER_INSTANCE as usize] = curve_indices();

const fn curve_indices() -> [u16; INDICES_PER_INSTANCE as usize] {
    let mut indices = [0; INDICES_PER_INSTANCE as usize];
    let mut segment = 0;
    while segment < SEGMENTS_PER_INSTANCE as u16 {
        let vertex = 2 * segment;
        let offset = 6 * segment as usize;
        indices[offset] = vertex;
        indices[offset + 1] = vertex + 2;
        indices[offset + 2] = vertex + 1;
        indices[offset + 3] = vertex + 1;
        indices[offset + 4] = vertex + 2;
        indices[offset + 5] = vertex + 3;
        segment += 1;
    }
    indices
}

const _: () = {
    assert!(INDICES_PER_INSTANCE == 96);
    assert!(UNIQUE_VERTICES_PER_INSTANCE == 34);
};

#[derive(Debug)]
pub(crate) struct CurvePipeline {
    instance_buffer: DynamicBuffer<CurveInstance>,
    index_buffer: wgpu::Buffer,
    /// Curve shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
}

impl CurvePipeline {
    /// Format-independent curve resources; the pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let wgsl = specialize(
            include_str!("curve.wgsl"),
            &[
                ShaderConstant::float("ATLAS_ROWS", ATLAS_ROWS as f32),
                ShaderConstant::uint("SEGMENTS_PER_INSTANCE", SEGMENTS_PER_INSTANCE),
                ShaderConstant::float("HALF_FRINGE", HALF_FRINGE),
                ShaderConstant::float("MITER_LIMIT", MITER_LIMIT),
                ShaderConstant::uint("CAP_BUTT", LineCap::Butt as u32),
                ShaderConstant::uint("CAP_SQUARE", LineCap::Square as u32),
                ShaderConstant::uint("CAP_ROUND", LineCap::Round as u32),
                ShaderConstant::uint("KIND_CUBIC", CURVE_KIND_CUBIC),
                ShaderConstant::uint("KIND_ARC", CURVE_KIND_ARC),
                ShaderConstant::uint("KIND_SEGMENT", CURVE_KIND_SEGMENT),
                ShaderConstant::uint("KIND_JOIN_ROUND", CURVE_KIND_JOIN_ROUND),
                ShaderConstant::uint("KIND_JOIN_BEVEL", CURVE_KIND_JOIN_BEVEL),
                ShaderConstant::uint("KIND_JOIN_MITER", CURVE_KIND_JOIN_MITER),
                ShaderConstant::uint("BRUSH_KIND_SOLID", FillKind::SOLID.0),
                ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::linear(Spread::Pad).0),
            ],
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.curve.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        let instance_buffer =
            DynamicBuffer::<CurveInstance>::vertex(device, "aperture.curve.instances", 64);
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aperture.curve.indices"),
            contents: bytemuck::cast_slice(&CURVE_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            instance_buffer,
            index_buffer,
            shader,
        }
    }

    /// Build the base + stencil-test color pipelines against `format`.
    /// Caller passes the shared `gradient_bgl` (owned by
    /// `GpuGradientAtlas`) so the layout matches; the instance buffer
    /// is format-independent. Called by `FormatPipelines` per format.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Gradient at group 0 — viewport rides the shared immediate
        // region, no bind-group slot needed for it.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.curve.pipeline",
                stencil_label: "aperture.curve.pipeline.stencil_test",
                layout_label: "aperture.curve.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(gradient_bgl)],
                vertex_buffers: &[Some(curve_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            format,
        )
    }

    #[profiling::function]
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[CurveInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    /// Bind once per pass, before issuing one [`Self::draw`] per
    /// curve group batch. Viewport rides the shared immediate region;
    /// `gradient_bg` is the group-0 handle owned by `GpuGradientAtlas`
    /// (one allocation, used by both the quad and curve pipelines).
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    }

    /// Issue one indexed instanced draw covering every instance in the
    /// span. This is the "one draw call per scissor group" terminus —
    /// the entire curve group batch lands as a single GPU draw call.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: std::ops::Range<u32>) {
        if instances.start == instances.end {
            return;
        }
        pass.draw_indexed(0..INDICES_PER_INSTANCE, 0, instances);
    }
}

// `p0/p1/p2/p3 : Float32x2`, `t_range : Float32x2`, `width : Float32`,
// `color0/color1 : Unorm8x4` (linear-u8, t=0 / t=1 stroke colours),
// `cap : Uint32` (per-end caps packed: bits 0..8 start, 8..16 end),
// `fill_kind : Uint32` (0 = solid, 1 = linear),
// `fill_lut_row : Uint32` (gradient atlas row when fill_kind != 0),
// `kind : Uint32` (basis tag — geometry-lane interpretation).
const CURVE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x2,
    4 => Float32x2,
    5 => Float32,
    6 => Unorm8x4,
    7 => Unorm8x4,
    8 => Uint32,
    9 => Uint32,
    10 => Uint32,
    11 => Uint32,
];

// Compile-time guard: attribute offsets must match the `CurveInstance`
// fields they feed. `array_stride == size_of` alone wouldn't catch a
// same-size field reorder or a format/field size mismatch; `offset_of!`
// does. Attr 4 (`Float32x2`) spans the adjacent `t0`,`t1` pair — anchored
// at `t0`, bracketed by the `width` check that follows.
const _: () = {
    use std::mem::offset_of;
    assert!(CURVE_INSTANCE_ATTRS[0].offset == offset_of!(CurveInstance, p0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[1].offset == offset_of!(CurveInstance, p1) as u64);
    assert!(CURVE_INSTANCE_ATTRS[2].offset == offset_of!(CurveInstance, p2) as u64);
    assert!(CURVE_INSTANCE_ATTRS[3].offset == offset_of!(CurveInstance, p3) as u64);
    assert!(CURVE_INSTANCE_ATTRS[4].offset == offset_of!(CurveInstance, t0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[5].offset == offset_of!(CurveInstance, width) as u64);
    assert!(CURVE_INSTANCE_ATTRS[6].offset == offset_of!(CurveInstance, color0) as u64);
    assert!(CURVE_INSTANCE_ATTRS[7].offset == offset_of!(CurveInstance, color1) as u64);
    assert!(CURVE_INSTANCE_ATTRS[8].offset == offset_of!(CurveInstance, cap) as u64);
    assert!(CURVE_INSTANCE_ATTRS[9].offset == offset_of!(CurveInstance, fill_kind) as u64);
    assert!(CURVE_INSTANCE_ATTRS[10].offset == offset_of!(CurveInstance, fill_lut_row) as u64);
    assert!(CURVE_INSTANCE_ATTRS[11].offset == offset_of!(CurveInstance, kind) as u64);
};

fn curve_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<CurveInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &CURVE_INSTANCE_ATTRS,
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::curve_pipeline::{
        CURVE_INDICES, INDICES_PER_INSTANCE, UNIQUE_VERTICES_PER_INSTANCE,
    };
    use crate::renderer::render_buffer::curve::SEGMENTS_PER_INSTANCE;

    #[test]
    fn curve_indices_tile_adjacent_cross_sections() {
        assert_eq!(CURVE_INDICES.len(), INDICES_PER_INSTANCE as usize);
        assert_eq!(
            CURVE_INDICES.iter().copied().max(),
            Some(UNIQUE_VERTICES_PER_INSTANCE - 1)
        );
        for segment in 0..SEGMENTS_PER_INSTANCE as u16 {
            let vertex = 2 * segment;
            let offset = 6 * segment as usize;
            assert_eq!(
                &CURVE_INDICES[offset..offset + 6],
                &[
                    vertex,
                    vertex + 2,
                    vertex + 1,
                    vertex + 1,
                    vertex + 2,
                    vertex + 3
                ],
                "segment {segment}"
            );
        }
    }
}
