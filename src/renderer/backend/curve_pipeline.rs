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

use super::UploadCtx;
use super::dynamic_buffer::DynamicBuffer;
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
    bind_group: wgpu::BindGroup,
    instance_buffer: DynamicBuffer,
    stencil_test: Option<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}

impl CurvePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
        gradient_texture_view: &wgpu::TextureView,
        gradient_sampler: &wgpu::Sampler,
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

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.curve.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.curve.bg"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(gradient_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(gradient_sampler),
                },
            ],
        });

        let pipeline_layout =
            build_pipeline_layout(device, "palantir.curve.pl", &[Some(&bind_layout)]);
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
            bind_group,
            instance_buffer,
            stencil_test: None,
            shader,
            color_format: format,
            bind_layout,
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    #[profiling::function]
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) {
        if self.stencil_test.is_some() {
            return;
        }
        let layout = build_pipeline_layout(
            device,
            "palantir.curve.pl.stencil",
            &[Some(&self.bind_layout)],
        );
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
    pub(crate) fn upload(&mut self, ctx: &mut UploadCtx<'_>, instances: &[CurveInstance]) {
        if instances.is_empty() {
            return;
        }
        self.instance_buffer
            .upload(ctx, bytemuck::cast_slice(instances), instances.len());
    }

    /// Bind once per pass, before issuing one [`Self::draw`] per
    /// `CurveBatch`.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.buffer().slice(..));
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
