//! The render-pipeline descriptor recipe every pipeline module builds through, so descriptor flags can't drift.

use crate::gpu::pipeline::IMMEDIATES_BYTES;

/// Render-pipeline recipe. Callers supply what varies (label, shader, layout, entry points, override constants, vertex buffers, topology, color format, writes, blend, optional depth-stencil); [`Self::build`] fills the project-wide defaults (single color target, no MSAA, no multiview). The built [`wgpu::RenderPipeline`] outlives the recipe.
#[derive(Debug)]
pub(crate) struct PipelineRecipe<'a> {
    pub(super) label: &'static str,
    pub(super) shader: &'a wgpu::ShaderModule,
    pub(super) layout: &'a wgpu::PipelineLayout,
    pub(super) vertex_entry: &'static str,
    /// Values for the shader's `override` constants, by name, in both stages; empty for defaults.
    pub(super) constants: &'a [(&'a str, f64)],
    pub(super) vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(super) topology: wgpu::PrimitiveTopology,
    pub(super) color_format: wgpu::TextureFormat,
    pub(super) fragment_entry: &'static str,
    pub(super) color_writes: wgpu::ColorWrites,
    pub(super) blend: Option<wgpu::BlendState>,
    pub(super) depth_stencil: Option<wgpu::DepthStencilState>,
}

impl PipelineRecipe<'_> {
    /// Build the pipeline. Sole source of the descriptor fields pipelines don't vary (sample count, multiview mask).
    pub(super) fn build(self, device: &wgpu::Device) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(self.label),
            layout: Some(self.layout),
            vertex: wgpu::VertexState {
                module: self.shader,
                entry_point: Some(self.vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: self.constants,
                    ..Default::default()
                },
                buffers: self.vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: self.shader,
                entry_point: Some(self.fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: self.constants,
                    ..Default::default()
                },
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

    /// Build the layout a recipe's [`Self::layout`] takes. Every pipeline declares the same immediate-region size, [`IMMEDIATES_BYTES`], taken by the prelude's `Immediates`.
    pub(crate) fn pipeline_layout(
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
