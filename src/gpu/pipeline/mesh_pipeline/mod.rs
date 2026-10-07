//! GPU side of user-supplied colored triangle meshes: like
//! [`crate::gpu::pipeline::quad_pipeline::QuadPipeline`] but indexed triangle
//! lists with per-vertex pos+color and per-instance transform+tint. The vertex
//! stream is content-stable across frames; per-draw state is in a parallel
//! instance buffer.
//!
//! **No `mesh_mask.wgsl`:** rounded-clip masks are quad-shaped and stamped by
//! [`QuadPipeline`](crate::gpu::pipeline::quad_pipeline::QuadPipeline) (`fs_mask`).
//! Mesh and [`crate::gpu::pipeline::image_pipeline::ImagePipeline`] only build a
//! stencil-*test* variant.

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::ColorVariantSpec;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::primitives::geometry::mesh::MeshVertex;
use crate::renderer::render_buffer::mesh::{MeshDraw, MeshInstance};

/// One frame's mesh geometry and per-draw state, uploaded together because an
/// instance indexes a draw's vertex and index spans.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshUpload<'a> {
    pub(crate) vertices: &'a [MeshVertex],
    pub(crate) indices: &'a [u32],
    pub(crate) instances: &'a [MeshInstance],
}

/// One batch of mesh draws: the frame's per-draw column and this batch's slice
/// of it. `items` is the same [`Span`] that indexes the instance buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshBatch<'a> {
    pub(crate) draws: &'a [MeshDraw],
    pub(crate) items: Span,
}

#[derive(Debug)]
pub(crate) struct MeshPipeline {
    vertex_buffer: DynamicBuffer<MeshVertex>,
    index_buffer: DynamicBuffer<u32>,
    instance_buffer: DynamicBuffer<MeshInstance>,
    /// Mesh shader module, format-independent; [`Self::build_variants`] builds each
    /// format's pipelines from it.
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
}

impl MeshPipeline {
    /// Format-independent mesh resources; pipelines come from
    /// [`Self::build_variants`] via
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = ShaderBody::Mesh.module(device);

        let vertex_buffer =
            DynamicBuffer::<MeshVertex>::vertex(device, "palantir.mesh.vertices", 256);
        let index_buffer = DynamicBuffer::<u32>::index(device, "palantir.mesh.indices", 1024);
        let instance_buffer =
            DynamicBuffer::<MeshInstance>::vertex(device, "palantir.mesh.instances", 64);

        Self {
            vertex_buffer,
            index_buffer,
            instance_buffer,
            shader,
            // No bind groups: only the shared immediate region for the viewport.
            pipeline_layout: PipelineRecipe::pipeline_layout(device, "palantir.mesh.pl", &[]),
        }
    }

    pub(super) const fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<MeshInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &MESH_INSTANCE_ATTRS,
        }
    }

    /// Build the base and stencil-test color pipelines against `format`, the only
    /// format-dependent mesh objects. Called by `FormatPipelines` per format.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.mesh.pipeline",
                stencil_label: "palantir.mesh.pipeline.stencil_test",
                shader: &self.shader,
                vertex_entry: "vs",
                fragment_entry: "fs",
                constants: &[],
                layout: &self.pipeline_layout,
                vertex_buffers: &[Some(mesh_vertex_layout()), Some(Self::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            format,
        )
    }

    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        MeshUpload {
            vertices,
            indices,
            instances,
        }: MeshUpload<'_>,
    ) {
        if !mesh_upload_required(vertices.len(), indices.len(), instances.len()) {
            return;
        }

        self.instance_buffer.upload_instances(ctx, instances);
        self.vertex_buffer.upload_instances(ctx, vertices);
        self.index_buffer.upload_instances(ctx, indices);
    }

    /// Bind pipeline and vertex/instance/index buffers once per batch; [`Self::draw`]
    /// then issues the draws. The viewport rides the shared immediate region,
    /// re-pushed by the backend's `rebind` after every pipeline switch.
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
    ) {
        pass.set_pipeline(variant.select(use_stencil));
        pass.set_vertex_buffer(0, self.vertex_buffer.buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.buffer.slice(..));
        pass.set_index_buffer(
            self.index_buffer.buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
    }

    /// Draw one mesh batch. `draws` is the frame's per-draw span column and `items`
    /// selects this batch's slice (the same `Span` that indexes the instance buffer).
    ///
    /// One `draw_indexed` per mesh, with no run to coalesce unlike `ImagePipeline`:
    /// `shapes::lower` appends each mesh's vertices and indices to the record
    /// payloads without interning, so no two `MeshDraw`s are equal. Interning by
    /// `content_hash` would be the prerequisite for batching.
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        MeshBatch { draws, items }: MeshBatch<'_>,
    ) {
        for (offset, draw) in draws[items.range()].iter().enumerate() {
            if draw.indices.len == 0 {
                continue;
            }
            let instance = items.start + offset as u32;
            pass.draw_indexed(
                draw.indices.into(),
                // Per-call vertex offset keeps a mesh's indices buffer-local.
                draw.vertices.start as i32,
                instance..instance + 1,
            );
        }
    }
}

fn mesh_upload_required(vertices: usize, indices: usize, instances: usize) -> bool {
    if instances == 0 {
        return false;
    }
    // Debug: these counts come from the composer a pass ago, so this checks the
    // crate at frame rate rather than screening caller input.
    debug_assert!(vertices != 0, "mesh instances require vertices");
    debug_assert!(indices != 0, "mesh instances require indices");
    true
}

const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x2,
    // `Unorm8x4` normalizes `u8/255` to 0..1. `MeshVertex::color` holds sRGB-encoded
    // bytes, decoded per vertex in the shader.
    1 => Unorm8x4,
];

// Attribute offsets must match the struct fields; `offset_of!` catches a
// same-size reorder that `array_stride == size_of` would miss.
const _: () = {
    use std::mem::offset_of;
    assert!(MESH_VERTEX_ATTRS[0].offset == offset_of!(MeshVertex, pos) as u64);
    assert!(MESH_VERTEX_ATTRS[1].offset == offset_of!(MeshVertex, color) as u64);
};

const fn mesh_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_VERTEX_ATTRS,
    }
}

// Tint is straight-alpha linear; the shader multiplies it by the decoded
// vertex colour.
const MESH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    2 => Float32x2,
    3 => Float32,
    4 => Float16x4,
];

const _: () = {
    use std::mem::offset_of;
    assert!(MESH_INSTANCE_ATTRS[0].offset == offset_of!(MeshInstance, translate) as u64);
    assert!(MESH_INSTANCE_ATTRS[1].offset == offset_of!(MeshInstance, scale) as u64);
    assert!(MESH_INSTANCE_ATTRS[2].offset == offset_of!(MeshInstance, tint) as u64);
};

#[cfg(test)]
mod tests {
    use super::mesh_upload_required;
    use crate::internals::panic_probe;

    #[test]
    fn mesh_upload_requires_geometry_only_when_instances_exist() {
        assert!(!mesh_upload_required(0, 0, 0));
        assert!(!mesh_upload_required(3, 3, 0));
        assert!(mesh_upload_required(3, 3, 1));
    }

    /// The two geometry screens are `debug_assert!`s: the composer produced these
    /// counts a pass ago and the upload path runs them at frame rate.
    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "probes a debug_assert!, which release compiles out"
    )]
    fn instances_without_geometry_are_screened() {
        panic_probe::assert_panics_with("mesh instances require vertices", || {
            mesh_upload_required(0, 3, 1)
        });
        panic_probe::assert_panics_with("mesh instances require indices", || {
            mesh_upload_required(3, 0, 1)
        });
    }
}
