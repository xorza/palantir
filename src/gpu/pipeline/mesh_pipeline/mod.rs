//! GPU side of user-supplied colored triangle meshes: like
//! [`crate::gpu::pipeline::quad_pipeline::QuadPipeline`] but indexed triangle
//! lists with per-vertex pos+color and per-instance transform+tint. The vertex
//! stream uploads each frame beside a parallel instance buffer of per-draw
//! state.
//!
//! **No `mesh_mask.wgsl`:** rounded-clip masks are quad-shaped and stamped by
//! [`QuadPipeline`](crate::gpu::pipeline::quad_pipeline::QuadPipeline) (`fs_mask`).
//! Mesh and [`crate::gpu::pipeline::image_pipeline::ImagePipeline`] only build a
//! stencil-*test* variant.

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::primitives::geometry::mesh::MeshVertex;
use crate::renderer::render_buffer::mesh_draw_row::{MeshDraw, MeshInstance};
use std::mem::offset_of;

/// One frame's mesh geometry and per-draw state, uploaded together because an
/// instance indexes a draw's vertex and index spans.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshUpload<'a> {
    pub(crate) vertices: &'a [MeshVertex],
    pub(crate) idxs: &'a [u32],
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
    idx_buffer: DynamicBuffer<u32>,
    instance_buffer: DynamicBuffer<MeshInstance>,
    /// Mesh shader module, format-independent; [`Self::build_variants`] builds each
    /// format's pipelines from it.
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
}

impl MeshPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.mesh.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[],
        constants: Vec::new,
    };

    /// Format-independent mesh resources; pipelines come from
    /// [`Self::build_variants`] via
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = Self::SHADER.module(device);

        let vertex_buffer =
            DynamicBuffer::<MeshVertex>::vertex(device, "palantir.mesh.vertices", 256);
        let idx_buffer = DynamicBuffer::<u32>::idx(device, "palantir.mesh.indices", 1024);
        let instance_buffer =
            DynamicBuffer::<MeshInstance>::vertex(device, "palantir.mesh.instances", 64);

        Self {
            vertex_buffer,
            idx_buffer,
            instance_buffer,
            shader,
            // No bind groups: only the shared immediate region for the viewport.
            pipeline_layout: pipeline_recipe::pipeline_layout(device, "palantir.mesh.pl", &[]),
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
            PipelineRecipe {
                vertex_buffers: &[Some(VERTEX_LAYOUT), Some(INSTANCE_LAYOUT)],
                ..PipelineRecipe::color(
                    "palantir.mesh.pipeline",
                    &self.shader,
                    &self.pipeline_layout,
                    format,
                )
            },
            "palantir.mesh.pipeline.stencil_test",
        )
    }

    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        MeshUpload {
            vertices,
            idxs,
            instances,
        }: MeshUpload<'_>,
    ) {
        if !mesh_upload_required(vertices.len(), idxs.len(), instances.len()) {
            return;
        }

        self.instance_buffer.upload_instances(ctx, instances);
        self.vertex_buffer.upload_instances(ctx, vertices);
        self.idx_buffer.upload_instances(ctx, idxs);
    }

    /// Bind pipeline and vertex/instance/index buffers once per batch; [`Self::draw`]
    /// then issues the draws. The viewport rides the shared immediate region,
    /// re-pushed by the backend's `rebind` after every pipeline switch.
    pub(crate) fn bind<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
    ) {
        binder.pipeline(pass, pipeline, &self.pipeline_layout);
        binder.vertex(pass, 0, &self.vertex_buffer.buffer);
        binder.vertex(pass, 1, &self.instance_buffer.buffer);
        binder.index(pass, &self.idx_buffer.buffer, wgpu::IndexFormat::Uint32);
    }

    /// Draw one mesh batch. `draws` is the frame's per-draw span column and `items`
    /// selects this batch's slice (the same `Span` that indexes the instance buffer).
    ///
    /// One `draw_indexed` per mesh, with no run to coalesce unlike `ImagePipeline`:
    /// `MeshShape`'s lowering appends each mesh's vertices and indices to the record
    /// payloads without interning, so no two `MeshDraw`s are equal. Interning by
    /// `content_hash` would be the prerequisite for batching.
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        MeshBatch { draws, items }: MeshBatch<'_>,
    ) {
        for (offset, draw) in draws[items.range()].iter().enumerate() {
            if draw.idxs.len == 0 {
                continue;
            }
            let instance = items.start + offset as u32;
            pass.draw_indexed(
                draw.idxs.into(),
                // Per-call vertex offset keeps a mesh's indices buffer-local.
                draw.vertices.start as i32,
                instance..instance + 1,
            );
        }
    }
}

fn mesh_upload_required(vertices: usize, idxs: usize, instances: usize) -> bool {
    if instances == 0 {
        return false;
    }
    // Debug: these counts come from the composer a pass ago, so this checks the
    // crate at frame rate rather than screening caller input.
    debug_assert!(vertices != 0, "mesh instances require vertices");
    debug_assert!(idxs != 0, "mesh instances require indices");
    true
}

const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Float32x2,
        offset_of!(MeshVertex, pos),
    ),
    // `Unorm8x4` normalizes `u8/255` to 0..1. `MeshVertex::color` holds sRGB-encoded
    // bytes, decoded per vertex in the shader.
    vertex_attr(
        1,
        wgpu::VertexFormat::Unorm8x4,
        offset_of!(MeshVertex, color),
    ),
];

const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<MeshVertex>(),
    wgpu::VertexStepMode::Vertex,
    &MESH_VERTEX_ATTRS,
);

// Tint is straight-alpha linear; the shader multiplies it by the decoded
// vertex color.
const MESH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = [
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32x2,
        offset_of!(MeshInstance, translate),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Float32x2,
        offset_of!(MeshInstance, rotor_scale),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Float16x4,
        offset_of!(MeshInstance, tint),
    ),
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<MeshInstance>(),
    wgpu::VertexStepMode::Instance,
    &MESH_INSTANCE_ATTRS,
);

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::mesh_pipeline::mesh_upload_required;
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
