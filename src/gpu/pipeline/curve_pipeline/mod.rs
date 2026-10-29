//! GPU side of native parametric strokes (cubic beziers, arcs, polyline segments, join billboards; see
//! `CurveInstance::kind`). One `draw_indexed` per scissor group covers its `GroupBatch`; an immutable index
//! buffer subdivides each instance into [`SEGMENTS_PER_INSTANCE`] chords. Same stencil-variant pattern as
//! [`MeshPipeline`] / [`ImagePipeline`].
//!
//! [`MeshPipeline`]: crate::gpu::pipeline::mesh_pipeline::MeshPipeline
//! [`ImagePipeline`]: crate::gpu::pipeline::image_pipeline::ImagePipeline

#[cfg(feature = "internals")]
pub(crate) mod bench;

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::shader_body::{GRADIENT, ShaderBody, ShaderConstant};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_instance::{CurveInstance, SEGMENTS_PER_INSTANCE};
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::shape::stroke_bounds::MITER_LIMIT;
use crate::shape::style::LineCap;
use std::mem::offset_of;
use wgpu::util::DeviceExt;

const IDXS_PER_INSTANCE: u32 = 6 * SEGMENTS_PER_INSTANCE;
const UNIQUE_VERTICES_PER_INSTANCE: u16 = 2 * (SEGMENTS_PER_INSTANCE as u16 + 1);
const CURVE_IDXS: [u16; IDXS_PER_INSTANCE as usize] = curve_idxs();

const fn curve_idxs() -> [u16; IDXS_PER_INSTANCE as usize] {
    let mut idxs = [0; IDXS_PER_INSTANCE as usize];
    let mut segment = 0;
    while segment < SEGMENTS_PER_INSTANCE as u16 {
        let vertex = 2 * segment;
        let offset = 6 * segment as usize;
        idxs[offset] = vertex;
        idxs[offset + 1] = vertex + 2;
        idxs[offset + 2] = vertex + 1;
        idxs[offset + 3] = vertex + 1;
        idxs[offset + 4] = vertex + 2;
        idxs[offset + 5] = vertex + 3;
        segment += 1;
    }
    idxs
}

const _: () = {
    assert!(IDXS_PER_INSTANCE == 96);
    assert!(UNIQUE_VERTICES_PER_INSTANCE == 34);
};

#[derive(Debug)]
pub(crate) struct CurvePipeline {
    instance_buffer: DynamicBuffer<CurveInstance>,
    idx_buffer: wgpu::Buffer,
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
}

impl CurvePipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.curve.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[GRADIENT],
        constants: shader_constants,
    };

    pub(crate) fn new(device: &wgpu::Device, textures: &TextureBinding) -> Self {
        let shader = Self::SHADER.module(device);

        let instance_buffer =
            DynamicBuffer::<CurveInstance>::vertex(device, "palantir.curve.instances", 64);
        let idx_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir.curve.indices"),
            contents: bytemuck::cast_slice(&CURVE_IDXS),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            instance_buffer,
            idx_buffer,
            shader,
            // Gradient at group 0; the viewport rides the shared immediate region.
            pipeline_layout: pipeline_recipe::pipeline_layout(
                device,
                "palantir.curve.pl",
                &[Some(textures.layout())],
            ),
        }
    }

    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        StencilVariant::build(
            device,
            PipelineRecipe {
                vertex_buffers: &[Some(INSTANCE_LAYOUT)],
                ..PipelineRecipe::color(
                    "palantir.curve.pipeline",
                    &self.shader,
                    &self.pipeline_layout,
                    format,
                )
            },
            "palantir.curve.pipeline.stencil_test",
        )
    }

    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[CurveInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    /// Binds once per pass, before one [`Self::draw`] per curve group batch.
    pub(crate) fn bind<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        gradient_bind_group: &'a wgpu::BindGroup,
    ) {
        binder.pipeline(pass, pipeline, &self.pipeline_layout);
        binder.group(pass, 0, gradient_bind_group);
        binder.vertex(pass, 0, &self.instance_buffer.buffer);
        binder.index(pass, &self.idx_buffer, wgpu::IndexFormat::Uint16);
    }

    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, instances: Span) {
        if instances.len == 0 {
            return;
        }
        pass.draw_indexed(0..IDXS_PER_INSTANCE, 0, instances.into());
    }
}

const CURVE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 12] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Float32x2,
        offset_of!(CurveInstance, p0),
    ),
    vertex_attr(
        1,
        wgpu::VertexFormat::Float32x2,
        offset_of!(CurveInstance, p1),
    ),
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32x2,
        offset_of!(CurveInstance, p2),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Float32x2,
        offset_of!(CurveInstance, p3),
    ),
    // Spans the adjacent `t0` and `t1`.
    vertex_attr(
        4,
        wgpu::VertexFormat::Float32x2,
        offset_of!(CurveInstance, t0),
    ),
    vertex_attr(
        5,
        wgpu::VertexFormat::Float32,
        offset_of!(CurveInstance, width),
    ),
    vertex_attr(
        6,
        wgpu::VertexFormat::Float16x4,
        offset_of!(CurveInstance, color0),
    ),
    vertex_attr(
        7,
        wgpu::VertexFormat::Float16x4,
        offset_of!(CurveInstance, color1),
    ),
    vertex_attr(
        8,
        wgpu::VertexFormat::Uint32,
        offset_of!(CurveInstance, caps),
    ),
    vertex_attr(
        9,
        wgpu::VertexFormat::Uint32,
        offset_of!(CurveInstance, fill_kind),
    ),
    vertex_attr(
        10,
        wgpu::VertexFormat::Uint32,
        offset_of!(CurveInstance, fill_lut_row),
    ),
    vertex_attr(
        11,
        wgpu::VertexFormat::Uint32,
        offset_of!(CurveInstance, kind),
    ),
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<CurveInstance>(),
    wgpu::VertexStepMode::Instance,
    &CURVE_INSTANCE_ATTRS,
);

/// The numbers [`CurvePipeline::SHADER`] takes from the Rust types that own them.
fn shader_constants() -> Vec<ShaderConstant> {
    vec![
        ShaderConstant::uint("SEGMENTS_PER_INSTANCE", SEGMENTS_PER_INSTANCE),
        ShaderConstant::float("MITER_LIMIT", MITER_LIMIT),
        ShaderConstant::uint("CAP_MASK", CurveCaps::CAP_MASK),
        ShaderConstant::uint("CAP_AT_START", CurveCaps::AT_START),
        ShaderConstant::uint("CAP_AT_END", CurveCaps::AT_END),
        ShaderConstant::uint("CAP_BUTT", LineCap::Butt as u32),
        ShaderConstant::uint("CAP_ROUND", LineCap::Round as u32),
        ShaderConstant::uint("KIND_ARC", CurveKind::ARC.bits()),
        ShaderConstant::uint("KIND_SEGMENT", CurveKind::SEGMENT.bits()),
        ShaderConstant::uint("KIND_JOIN_ROUND", CurveKind::JOIN_ROUND.bits()),
        ShaderConstant::uint("KIND_JOIN_BEVEL", CurveKind::JOIN_BEVEL.bits()),
        ShaderConstant::uint("KIND_JOIN_MITER", CurveKind::JOIN_MITER.bits()),
        ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
        ShaderConstant::uint("BRUSH_KIND_RAMP", FillKind::TAG_RAMP),
    ]
}

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::curve_pipeline::{
        CURVE_IDXS, IDXS_PER_INSTANCE, UNIQUE_VERTICES_PER_INSTANCE,
    };
    use crate::renderer::render_buffer::curve_instance::SEGMENTS_PER_INSTANCE;

    /// Segment `s` joins cross-sections `s` and `s + 1`, vertices `2s, 2s + 1` and `2s + 2, 2s + 3`,
    /// as two triangles of one winding: 16 segments over 34 vertices, the first two and the last
    /// written out.
    #[test]
    fn curve_indices_tile_adjacent_cross_sections() {
        assert_eq!(SEGMENTS_PER_INSTANCE, 16);
        assert_eq!(IDXS_PER_INSTANCE, 96);
        assert_eq!(UNIQUE_VERTICES_PER_INSTANCE, 34);
        assert_eq!(&CURVE_IDXS[..12], &[0, 2, 1, 1, 2, 3, 2, 4, 3, 3, 4, 5]);
        assert_eq!(&CURVE_IDXS[90..], &[30, 32, 31, 31, 32, 33]);
        assert_eq!(CURVE_IDXS.iter().copied().max(), Some(33));
    }
}
