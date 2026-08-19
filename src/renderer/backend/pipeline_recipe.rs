//! The render-pipeline descriptor recipe every pipeline module builds
//! through, so they cannot drift on descriptor flags.

use crate::renderer::backend::IMMEDIATES_BYTES;

/// Render-pipeline recipe. Threads the call-site fields each pipeline
/// genuinely varies (label, shader, layout, vertex buffers, topology,
/// color format, fragment entry, color writes, blend, optional
/// depth-stencil) and lets [`Self::build`] fill in the rest with
/// the project-wide defaults (single color target, no MSAA, no
/// multiview, vertex entry = `"vs"`).
///
/// `'a` is the lifetime of the references passed in; the returned
/// [`wgpu::RenderPipeline`] retains its own internal references and
/// outlives the recipe.
#[derive(Debug)]
pub(super) struct PipelineRecipe<'a> {
    pub label: &'static str,
    pub shader: &'a wgpu::ShaderModule,
    pub layout: &'a wgpu::PipelineLayout,
    pub vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub topology: wgpu::PrimitiveTopology,
    pub color_format: wgpu::TextureFormat,
    pub fragment_entry: &'static str,
    pub color_writes: wgpu::ColorWrites,
    pub blend: Option<wgpu::BlendState>,
    pub depth_stencil: Option<wgpu::DepthStencilState>,
}

impl PipelineRecipe<'_> {
    /// Build the render pipeline this recipe describes. Sole source of
    /// truth for the descriptor fields each pipeline doesn't vary —
    /// vertex entry, sample count, multiview mask. Every quad / mesh /
    /// image / curve / text pipeline goes through here.
    pub(super) fn build(self, device: &wgpu::Device) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(self.label),
            layout: Some(self.layout),
            vertex: wgpu::VertexState {
                module: self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: self.vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: self.shader,
                entry_point: Some(self.fragment_entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: self.blend,
                    write_mask: self.color_writes,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: self.topology,
                ..Default::default()
            },
            depth_stencil: self.depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Build the pipeline layout a recipe's [`Self::layout`] field takes.
    /// Every palantir pipeline declares the same immediate-region size
    /// ([`IMMEDIATES_BYTES`]) so the immediate state set by the backend at
    /// pass open (viewport) stays valid as pipelines switch, and the text
    /// pipeline can additionally write its `Params` at offset 8.
    pub(super) fn pipeline_layout(
        device: &wgpu::Device,
        label: &'static str,
        bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    ) -> wgpu::PipelineLayout {
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts,
            immediate_size: IMMEDIATES_BYTES,
        })
    }
}
