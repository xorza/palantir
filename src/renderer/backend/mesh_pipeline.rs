//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`crate::renderer::backend::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color and per-instance
//! transform+tint. The vertex stream is content-stable across frames;
//! per-draw state lives in a parallel instance buffer.
//!
//! **No `mesh_mask.wgsl`.** Rounded-clip masks are quad-shaped and
//! always stamped by [`QuadPipeline`]'s `mask_write` variant
//! (`quad.wgsl::fs_mask`). Mesh only builds a stencil-*test* variant
//! (see [`Self::ensure_stencil`]) — it reads the mask but never
//! writes one. Same shape for [`crate::renderer::backend::image_pipeline::ImagePipeline`].

use crate::primitives::mesh::MeshVertex;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{
    PipelineRecipe, StencilVariant, build_pipeline, build_pipeline_layout,
};
use crate::renderer::backend::stencil::stencil_test_state;
use crate::renderer::render_buffer::MeshInstance;

pub(crate) struct MeshPipeline {
    vertex_buffer: DynamicBuffer,
    index_buffer: DynamicBuffer,
    instance_buffer: DynamicBuffer,
    /// Mesh shader module — format-independent; `FormatPipelines` reads it
    /// to build this format's pipelines.
    pub(crate) shader: wgpu::ShaderModule,
    /// Retained scratch for the odd-length index pad-to-even path; one
    /// upload instead of two belt writes. Capacity retained across frames.
    index_scratch: Vec<u16>,
}

impl MeshPipeline {
    /// Format-independent mesh resources; the pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });

        let vertex_buffer =
            DynamicBuffer::vertex::<MeshVertex>(device, "palantir.mesh.vertices", 256);
        let index_buffer = DynamicBuffer::index::<u16>(device, "palantir.mesh.indices", 1024);
        let instance_buffer =
            DynamicBuffer::vertex::<MeshInstance>(device, "palantir.mesh.instances", 64);

        Self {
            vertex_buffer,
            index_buffer,
            instance_buffer,
            shader,
            index_scratch: Vec::new(),
        }
    }

    /// Build the color pipeline against `format` — the only
    /// format-dependent object; the vertex / index / instance buffers
    /// are reused. `stencil` selects the rounded-clip variant (adds the
    /// shared `stencil_test_state`). Called by `FormatPipelines` per
    /// format.
    pub(crate) fn build_variant(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        color_format: wgpu::TextureFormat,
        stencil: bool,
    ) -> wgpu::RenderPipeline {
        let (label, layout_label, depth_stencil) = if stencil {
            (
                "palantir.mesh.pipeline.stencil_test",
                "palantir.mesh.pl.stencil",
                Some(stencil_test_state()),
            )
        } else {
            ("palantir.mesh.pipeline", "palantir.mesh.pl", None)
        };
        // Mesh shader uses no bind groups — only the shared immediate
        // region for viewport. Empty bind-group-layout list.
        let layout = build_pipeline_layout(device, layout_label, &[]);
        build_pipeline(
            device,
            PipelineRecipe {
                label,
                shader,
                layout: &layout,
                vertex_buffers: &[mesh_vertex_layout(), mesh_instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleList,
                color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil,
            },
        )
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
        // odd-length: copy into a retained scratch with a trailing 0 and
        // do a single upload.
        let padded = (indices.len() + 1) & !1;
        if indices.len() == padded {
            self.index_buffer
                .upload(ctx, bytemuck::cast_slice(indices), padded);
        } else {
            self.index_scratch.clear();
            self.index_scratch.extend_from_slice(indices);
            self.index_scratch.push(0);
            self.index_buffer
                .upload(ctx, bytemuck::cast_slice(&self.index_scratch), padded);
        }
    }

    /// Bind once per pass, before iterating `meshes` and issuing
    /// `draw_range` per group entry. The shared viewport bind group
    /// at slot 0 is set once per pass by `WgpuBackend::run_main_pass`
    /// — switching pipelines doesn't invalidate it because all four
    /// pipelines share the same `@group(0)` layout.
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
    ) {
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_vertex_buffer(0, self.vertex_buffer.buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.buffer.slice(..));
        pass.set_index_buffer(
            self.index_buffer.buffer.slice(..),
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

// Compile-time guard: attribute offsets must match the struct fields they
// feed. `array_stride == size_of` alone wouldn't catch a same-size field
// reorder or a format/field size mismatch; `offset_of!` does.
const _: () = {
    use std::mem::offset_of;
    assert!(MESH_VERTEX_ATTRS[0].offset == offset_of!(MeshVertex, pos) as u64);
    assert!(MESH_VERTEX_ATTRS[1].offset == offset_of!(MeshVertex, color) as u64);
};

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

const _: () = {
    use std::mem::offset_of;
    assert!(MESH_INSTANCE_ATTRS[0].offset == offset_of!(MeshInstance, translate) as u64);
    assert!(MESH_INSTANCE_ATTRS[1].offset == offset_of!(MeshInstance, scale) as u64);
    assert!(MESH_INSTANCE_ATTRS[2].offset == offset_of!(MeshInstance, tint) as u64);
};

fn mesh_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &MESH_INSTANCE_ATTRS,
    }
}
