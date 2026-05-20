//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`super::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color and per-instance
//! transform+tint. The vertex stream is content-stable across frames;
//! per-draw state lives in a parallel instance buffer.
//!
//! **No `mesh_mask.wgsl`.** Rounded-clip masks are quad-shaped and
//! always stamped by [`QuadPipeline`]'s `mask_write` variant
//! (`quad.wgsl::fs_mask`). Mesh only builds a stencil-*test* variant
//! (see [`Self::ensure_stencil`]) — it reads the mask but never
//! writes one. Same shape for [`super::image_pipeline::ImagePipeline`].

use super::GpuCtx;
use super::dynamic_buffer::DynamicBuffer;
use super::pipeline_utils::{PipelineRecipe, build_pipeline, build_pipeline_layout};
use crate::primitives::mesh::MeshVertex;
use crate::renderer::render_buffer::MeshInstance;

pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertex_buffer: DynamicBuffer,
    index_buffer: DynamicBuffer,
    instance_buffer: DynamicBuffer,
    stencil_test: Option<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}

impl MeshPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.mesh.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.mesh.bg"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout =
            build_pipeline_layout(device, "palantir.mesh.pl", &[Some(&bind_layout)]);
        let pipeline = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.mesh.pipeline",
                shader: &shader,
                layout: &pipeline_layout,
                vertex_buffers: &[mesh_vertex_layout(), mesh_instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleList,
                color_format: format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: None,
            },
        );

        let vertex_buffer =
            DynamicBuffer::vertex::<MeshVertex>(device, "palantir.mesh.vertices", 256, 64);
        let index_buffer = DynamicBuffer::index::<u16>(device, "palantir.mesh.indices", 1024, 256);
        let instance_buffer =
            DynamicBuffer::vertex::<MeshInstance>(device, "palantir.mesh.instances", 64, 16);

        Self {
            pipeline,
            bind_group,
            vertex_buffer,
            index_buffer,
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
            "palantir.mesh.pl.stencil",
            &[Some(&self.bind_layout)],
        );
        self.stencil_test = Some(build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.mesh.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: &[mesh_vertex_layout(), mesh_instance_layout()],
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
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        vertices: &[MeshVertex],
        indices: &[u16],
        instances: &[MeshInstance],
    ) {
        if vertices.is_empty() || indices.is_empty() || instances.is_empty() {
            return;
        }

        self.instance_buffer
            .upload(ctx, bytemuck::cast_slice(instances), instances.len());
        self.vertex_buffer
            .upload(ctx, bytemuck::cast_slice(vertices), vertices.len());

        // The index buffer's binding stride is 2 bytes (u16). wgpu
        // requires copy size to be a multiple of 4 (COPY_BUFFER_ALIGNMENT),
        // so pad the upload to an even count when the index list is
        // odd-length: write the even prefix + a single padded tail u16.
        // Hash incorporates the canonical padded form so the gate
        // matches whether the same content arrives via the even or odd
        // path next frame.
        let padded = (indices.len() + 1) & !1;
        if indices.len() == padded {
            self.index_buffer
                .upload(ctx, bytemuck::cast_slice(indices), padded);
        } else {
            use std::hash::Hasher as _;
            let mut h = crate::common::hash::Hasher::new();
            h.write(bytemuck::cast_slice(indices));
            h.write_u16(0);
            let content_hash = h.finish();
            self.index_buffer
                .upload_with(ctx, padded, content_hash, |buf, ctx| {
                    ctx.write(buf, 0, bytemuck::cast_slice(&indices[..indices.len() - 1]));
                    let tail = [indices[indices.len() - 1], 0u16];
                    ctx.write(
                        buf,
                        ((indices.len() - 1) * std::mem::size_of::<u16>()) as u64,
                        bytemuck::cast_slice(&tail),
                    );
                });
        }
    }

    /// Bind once per pass, before iterating `meshes` and issuing
    /// `draw_range` per group entry.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.buffer().slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.buffer().slice(..));
        pass.set_index_buffer(
            self.index_buffer.buffer().slice(..),
            wgpu::IndexFormat::Uint16,
        );
    }

    /// Issue one indexed draw for a single [`MeshDraw`](crate::renderer::render_buffer::MeshDraw).
    /// `instance` indexes into the per-frame instance buffer for the
    /// matching transform + tint.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        index_range: std::ops::Range<u32>,
        base_vertex: i32,
        instance: u32,
    ) {
        if index_range.start == index_range.end {
            return;
        }
        pass.draw_indexed(index_range, base_vertex, instance..instance + 1);
    }
}

const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x2,
    // `Unorm8x4` normalizes `u8/255 → 0..1` floats on the GPU. The
    // CPU side stores linear-u8 via the linear `From<Color> for
    // ColorU8` impl, so the shader sees linear values directly —
    // no decode, no banding worse than 1/255 (below display step).
    1 => Unorm8x4,
];

fn mesh_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_VERTEX_ATTRS,
    }
}

// `translate.xy : Float32x2`, `scale : Float32`, `tint : Unorm8x4`.
// Tint storage matches `MeshVertex.color` (linear-u8 premultiplied);
// shader multiplies per-fragment, no decode either side.
const MESH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    2 => Float32x2,
    3 => Float32,
    4 => Unorm8x4,
];

fn mesh_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &MESH_INSTANCE_ATTRS,
    }
}
