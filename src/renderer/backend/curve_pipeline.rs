//! GPU side of native bezier-curve strokes. One `draw` per scissor
//! group covers every `CurveInstance` in the group's `CurveBatch` —
//! the vertex shader subdivides each instance into
//! [`SEGMENTS_PER_INSTANCE`](crate::renderer::frontend::composer::SEGMENTS_PER_INSTANCE)
//! chords (96 vertices per instance, no index buffer) and offsets the
//! strip perpendicular to the tangent for stroking + AA.
//!
//! Same lazy-stencil-variant pattern as [`MeshPipeline`] /
//! [`ImagePipeline`]: rounded-clip frames use a stencil-test pipeline,
//! plain frames use the unconditional one.
//!
//! [`MeshPipeline`]: super::mesh_pipeline::MeshPipeline
//! [`ImagePipeline`]: super::image_pipeline::ImagePipeline

use super::dynamic_buffer::DynamicBuffer;
use super::gpu_ctx::GpuCtx;
use super::pipeline_utils::{PipelineRecipe, build_pipeline, build_pipeline_layout};
use crate::renderer::frontend::composer::SEGMENTS_PER_INSTANCE;
use crate::renderer::render_buffer::CurveInstance;
use crate::shape::LineCap;

/// Vertex count per instance — every instance is a 16-segment strip,
/// 6 vertices per segment (two triangles), no index buffer.
const VERTICES_PER_INSTANCE: u32 = 6 * SEGMENTS_PER_INSTANCE;

// Pin the LineCap discriminants against the `CAP_*` constants in
// `curve.wgsl`. Reorder a `LineCap` variant without updating the
// shader and curves silently mis-render (Butt becomes Square, etc.).
// Build fails here before that ships.
const _: () = {
    assert!(LineCap::Butt as u8 == 0);
    assert!(LineCap::Square as u8 == 1);
    assert!(LineCap::Round as u8 == 2);
};

pub(crate) struct CurvePipeline {
    pipeline: wgpu::RenderPipeline,
    instance_buffer: DynamicBuffer,
    stencil_test: Option<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
}

impl CurvePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        gradient_bgl: &wgpu::BindGroupLayout,
    ) -> Self {
        // Stamp the Rust-side `SEGMENTS_PER_INSTANCE` into the WGSL
        // source so the shader can't drift out of lockstep with the
        // composer's sub-instance math. Cheap one-time string op at
        // pipeline creation; no per-frame cost.
        let wgsl = include_str!("curve.wgsl").replace(
            "/*{SEGMENTS_PER_INSTANCE}*/16u",
            &format!("{SEGMENTS_PER_INSTANCE}u"),
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.curve.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        // Gradient at group 0 — viewport rides the shared immediate
        // region, no bind-group slot needed for it.
        let pipeline_layout =
            build_pipeline_layout(device, "palantir.curve.pl", &[Some(gradient_bgl)]);
        let pipeline = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.curve.pipeline",
                shader: &shader,
                layout: &pipeline_layout,
                vertex_buffers: &[curve_instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleList,
                color_format: format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: None,
            },
        );

        let instance_buffer =
            DynamicBuffer::vertex::<CurveInstance>(device, "palantir.curve.instances", 64, 64);

        Self {
            pipeline,
            instance_buffer,
            stencil_test: None,
            shader,
            color_format: format,
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    /// Caller passes the shared `gradient_bgl` (owned by the quad
    /// pipeline) so the variant matches the base pipeline's layout.
    #[profiling::function]
    pub(crate) fn ensure_stencil(
        &mut self,
        device: &wgpu::Device,
        gradient_bgl: &wgpu::BindGroupLayout,
    ) {
        if self.stencil_test.is_some() {
            return;
        }
        let layout =
            build_pipeline_layout(device, "palantir.curve.pl.stencil", &[Some(gradient_bgl)]);
        self.stencil_test = Some(build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.curve.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: &[curve_instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleList,
                color_format: self.color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: Some(super::stencil::stencil_test_state()),
            },
        ));
    }

    #[profiling::function]
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[CurveInstance]) {
        if instances.is_empty() {
            return;
        }
        self.instance_buffer
            .upload(ctx, bytemuck::cast_slice(instances), instances.len());
    }

    /// Bind once per pass, before issuing one [`Self::draw`] per
    /// `CurveBatch`. Viewport rides the shared immediate region;
    /// `gradient_bg` is the group-0 handle owned by `QuadPipeline`
    /// (one allocation, used by both pipelines).
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        stencil: bool,
        gradient_bg: &'a wgpu::BindGroup,
    ) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, gradient_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.buffer.slice(..));
    }

    /// Issue one non-indexed instanced draw covering every instance in
    /// the span (no index buffer — `vertex_index` maps directly to the
    /// 6 corners of each of `SEGMENTS_PER_INSTANCE` quads). This is the
    /// "one draw call per scissor group" terminus — the entire
    /// `CurveBatch` lands as a single GPU draw call.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: std::ops::Range<u32>) {
        if instances.start == instances.end {
            return;
        }
        pass.draw(0..VERTICES_PER_INSTANCE, instances);
    }
}

// `p0/p1/p2/p3 : Float32x2`, `t_range : Float32x2`, `width : Float32`,
// `color : Unorm8x4` (linear-u8, same convention as `MeshVertex.color`),
// `cap : Uint32` (0 = Butt, 1 = Square, 2 = Round),
// `fill_kind : Uint32` (0 = solid, 1 = linear),
// `fill_lut_row : Uint32` (gradient atlas row when fill_kind != 0).
const CURVE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x2,
    4 => Float32x2,
    5 => Float32,
    6 => Unorm8x4,
    7 => Uint32,
    8 => Uint32,
    9 => Uint32,
];

fn curve_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<CurveInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &CURVE_INSTANCE_ATTRS,
    }
}
