//! The render-pipeline descriptor recipe every pipeline module builds through, so descriptor flags can't drift.

use crate::gpu::pipeline::IMMEDIATES_BYTES;

/// Render-pipeline recipe. Callers supply what varies (label, shader, layout, entry points, override constants, vertex buffers, topology, color format, writes, blend, optional depth-stencil); [`Self::build`] fills the project-wide defaults (single color target, no MSAA, no multiview). The built [`wgpu::RenderPipeline`] outlives the recipe.
#[derive(Clone, Debug)]
pub(crate) struct PipelineRecipe<'a> {
    pub(crate) label: &'static str,
    pub(crate) shader: &'a wgpu::ShaderModule,
    pub(crate) layout: &'a wgpu::PipelineLayout,
    pub(crate) vertex_entry: &'static str,
    /// Values for the shader's `override` constants, by name, in both stages; empty for defaults.
    pub(crate) constants: &'a [(&'a str, f64)],
    pub(crate) vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(crate) topology: wgpu::PrimitiveTopology,
    pub(crate) color_format: wgpu::TextureFormat,
    pub(crate) fragment_entry: &'static str,
    pub(crate) color_writes: wgpu::ColorWrites,
    pub(crate) blend: Option<wgpu::BlendState>,
    pub(crate) depth_stencil: Option<wgpu::DepthStencilState>,
}

impl<'a> PipelineRecipe<'a> {
    /// A drawing family's color pipeline, the shape every one starts from: `vs` and `fs`, no
    /// override constants or vertex buffers, a triangle list, every channel written, premultiplied
    /// blending, no stencil. A family overrides what it varies with struct update.
    pub(crate) const fn color(
        label: &'static str,
        shader: &'a wgpu::ShaderModule,
        layout: &'a wgpu::PipelineLayout,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            label,
            shader,
            layout,
            vertex_entry: "vs",
            constants: &[],
            vertex_buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            color_format,
            fragment_entry: "fs",
            color_writes: wgpu::ColorWrites::ALL,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            depth_stencil: None,
        }
    }

    /// Build the pipeline. Sole source of the descriptor fields pipelines don't vary (sample count, multiview mask).
    pub(crate) fn build(self, device: &wgpu::Device) -> wgpu::RenderPipeline {
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
