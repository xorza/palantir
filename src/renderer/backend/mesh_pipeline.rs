//! GPU side of user-supplied colored triangle meshes. Mirrors
//! [`crate::renderer::backend::quad_pipeline::QuadPipeline`] but draws indexed
//! triangle lists with per-vertex pos+color and per-instance
//! transform+tint. The vertex stream is content-stable across frames;
//! per-draw state lives in a parallel instance buffer.
//!
//! **No `mesh_mask.wgsl`.** Rounded-clip masks are quad-shaped and
//! always stamped by [`QuadPipeline`](crate::renderer::backend::quad_pipeline::QuadPipeline)'s
//! mask stamp/clear variants
//! (`quad.wgsl::fs_mask`). Mesh only builds a stencil-*test* variant —
//! it reads the mask but never writes one. Same shape for
//! [`crate::renderer::backend::image_pipeline::ImagePipeline`].

use crate::primitives::mesh::MeshVertex;
use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::instance_pipeline::InstancePipeline;
use crate::renderer::backend::shader_template;
use crate::renderer::backend::stencil_variant::ColorVariantSpec;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::render_buffer::mesh::{MeshDraw, MeshInstance};

/// One frame's mesh geometry and per-draw state, uploaded together.
///
/// The three streams have to agree — an instance indexes a draw's vertex
/// and index spans — so they travel as one value rather than three
/// adjacent slice parameters.
#[derive(Clone, Copy, Debug)]
pub(super) struct MeshUpload<'a> {
    pub(super) vertices: &'a [MeshVertex],
    pub(super) indices: &'a [u32],
    pub(super) instances: &'a [MeshInstance],
}

/// One batch of mesh draws: the frame's whole per-draw column, and the
/// slice of it this batch owns.
///
/// `items` is the same [`Span`] that indexes the per-frame instance
/// buffer, so a draw's geometry and its transform plus tint cannot come
/// from different batches.
#[derive(Clone, Copy, Debug)]
pub(super) struct MeshBatch<'a> {
    pub(super) draws: &'a [MeshDraw],
    pub(super) items: Span,
}

#[derive(Debug)]
pub(super) struct MeshPipeline {
    vertex_buffer: DynamicBuffer<MeshVertex>,
    index_buffer: DynamicBuffer<u32>,
    instance_buffer: DynamicBuffer<MeshInstance>,
    /// Mesh shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
}

impl InstancePipeline for MeshPipeline {
    /// None past the device: mesh binds no groups at all.
    type Layouts<'a> = ();
    type Upload<'a> = MeshUpload<'a>;
    type Bindings<'a> = ();
    type Batch<'a> = MeshBatch<'a>;

    /// Format-independent mesh resources; the pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(
                shader_template::specialize(shader_template::MESH_WGSL, &[]).into(),
            ),
        });

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
        }
    }

    fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &MESH_INSTANCE_ATTRS,
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent mesh objects; the vertex / index /
    /// instance buffers are reused. Called by `FormatPipelines` per
    /// format.
    fn build_variants(
        &self,
        device: &wgpu::Device,
        _layouts: Self::Layouts<'_>,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Mesh shader uses no bind groups — only the shared immediate
        // region for viewport. Empty bind-group-layout list.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.mesh.pipeline",
                stencil_label: "palantir.mesh.pipeline.stencil_test",
                layout_label: "palantir.mesh.pl",
                shader: &self.shader,
                bind_group_layouts: &[],
                vertex_buffers: &[Some(mesh_vertex_layout()), Some(Self::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            format,
        )
    }

    fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        MeshUpload {
            vertices,
            indices,
            instances,
        }: Self::Upload<'_>,
    ) {
        if !mesh_upload_required(vertices.len(), indices.len(), instances.len()) {
            return;
        }

        self.instance_buffer.upload_instances(ctx, instances);
        self.vertex_buffer.upload_instances(ctx, vertices);
        self.index_buffer.upload_instances(ctx, indices);
    }

    /// Bind pipeline + vertex/instance/index buffers once per batch;
    /// [`Self::draw`] then issues the draws. Mesh binds no groups —
    /// the viewport rides the shared immediate region, re-pushed by
    /// the backend's `rebind!` after every pipeline switch.
    fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
        _bindings: Self::Bindings<'a>,
    ) {
        pass.set_pipeline(variant.select(use_stencil));
        pass.set_vertex_buffer(0, self.vertex_buffer.buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.buffer.slice(..));
        pass.set_index_buffer(
            self.index_buffer.buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
    }

    /// Draw one mesh batch. `draws` is the frame's whole per-draw span
    /// column and `items` selects this batch's slice of it — the same
    /// `Span` that indexes the per-frame instance buffer, so a draw's
    /// geometry and its transform + tint cannot come from different
    /// batches.
    ///
    /// One `draw_indexed` per mesh, and unlike `ImagePipeline`'s arm of
    /// the same method there is no run to coalesce: `shapes::lower` appends each authored
    /// mesh's vertices and indices to the record payloads rather than
    /// interning them, so every draw owns a private span and no two
    /// `MeshDraw`s are ever equal — not even two draws of the same
    /// `Mesh`. Interning payloads by their already-computed
    /// `content_hash` would change that, and is the prerequisite for any
    /// batching here.
    fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        MeshBatch { draws, items }: Self::Batch<'_>,
    ) {
        for (offset, draw) in draws[items.range()].iter().enumerate() {
            if draw.indices.len == 0 {
                continue;
            }
            // The draw's absolute slot in `meshes.instances`.
            let instance = items.start + offset as u32;
            pass.draw_indexed(
                draw.indices.into(),
                // Per-call vertex offset, so a mesh's indices stay
                // buffer-local rather than needing a rebase at record time.
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
    // Debug: the composer produced these counts a pass ago and this runs
    // on every frame's upload path, so it is the crate checking itself at
    // frame rate rather than screening anything a caller passed.
    debug_assert!(vertices != 0, "mesh instances require vertices");
    debug_assert!(indices != 0, "mesh instances require indices");
    true
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

#[cfg(test)]
mod tests {
    use super::mesh_upload_required;

    #[test]
    fn mesh_upload_requires_geometry_only_when_instances_exist() {
        assert!(!mesh_upload_required(0, 0, 0));
        assert!(!mesh_upload_required(3, 3, 0));
        assert!(mesh_upload_required(3, 3, 1));

        assert!(std::panic::catch_unwind(|| mesh_upload_required(0, 3, 1)).is_err());
        assert!(std::panic::catch_unwind(|| mesh_upload_required(3, 0, 1)).is_err());
    }
}
