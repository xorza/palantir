//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`crate::renderer::backend::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color and per-instance
//! transform+tint. The vertex stream is content-stable across frames;
//! per-draw state lives in a parallel instance buffer.
//!
//! **No `mesh_mask.wgsl`.** Rounded-clip masks are quad-shaped and
//! always stamped by [`QuadPipeline`]'s mask stamp/clear variants
//! (`quad.wgsl::fs_mask`). Mesh only builds a stencil-*test* variant —
//! it reads the mask but never writes one. Same shape for
//! [`crate::renderer::backend::image_pipeline::ImagePipeline`].

use crate::primitives::mesh::MeshVertex;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::render_buffer::MeshInstance;

#[derive(Debug)]
pub(crate) struct MeshPipeline {
    vertex_buffer: DynamicBuffer,
    index_buffer: DynamicBuffer,
    instance_buffer: DynamicBuffer,
    /// Mesh shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
}

impl MeshPipeline {
    /// Format-independent mesh resources; the pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });

        let vertex_buffer =
            DynamicBuffer::vertex::<MeshVertex>(device, "aperture.mesh.vertices", 256);
        let index_buffer = DynamicBuffer::index::<u32>(device, "aperture.mesh.indices", 1024);
        let instance_buffer =
            DynamicBuffer::vertex::<MeshInstance>(device, "aperture.mesh.instances", 64);

        Self {
            vertex_buffer,
            index_buffer,
            instance_buffer,
            shader,
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent mesh objects; the vertex / index /
    /// instance buffers are reused. Called by `FormatPipelines` per
    /// format.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Mesh shader uses no bind groups — only the shared immediate
        // region for viewport. Empty bind-group-layout list.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.mesh.pipeline",
                stencil_label: "aperture.mesh.pipeline.stencil_test",
                layout_label: "aperture.mesh.pl",
                shader: &self.shader,
                bind_group_layouts: &[],
                vertex_buffers: &[Some(mesh_vertex_layout()), Some(mesh_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            format,
        )
    }

    #[profiling::function]
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        vertices: &[MeshVertex],
        indices: &[u32],
        instances: &[MeshInstance],
    ) {
        // Joint guard: a frame missing any of the three slices can't
        // draw a mesh, so skip all uploads rather than land partial
        // buffers.
        if vertices.is_empty() || indices.is_empty() || instances.is_empty() {
            return;
        }

        self.instance_buffer.upload_instances(ctx, instances);
        self.vertex_buffer.upload_instances(ctx, vertices);
        self.index_buffer.upload_instances(ctx, indices);
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
            wgpu::IndexFormat::Uint32,
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
// Tint storage matches `MeshVertex.color` (straight-alpha linear-u8);
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
