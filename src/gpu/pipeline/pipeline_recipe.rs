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
}

/// Build the layout a recipe's [`PipelineRecipe::layout`] takes. Every pipeline declares the same immediate-region size, [`IMMEDIATES_BYTES`], taken by the prelude's `Immediates`.
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

/// An attribute at shader `location` reading `format` at byte `offset` of its vertex: an
/// `offset_of!`, so the attribute follows its field.
pub(crate) const fn vertex_attr(
    location: u32,
    format: wgpu::VertexFormat,
    offset: usize,
) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        format,
        offset: offset as u64,
        shader_location: location,
    }
}

/// The layout of a buffer of `stride`-byte vertices that steps by `step_mode`, read through
/// `attributes`. Built in a `const`, an attribute that overruns its field fails the build.
pub(crate) const fn vertex_layout(
    stride: usize,
    step_mode: wgpu::VertexStepMode,
    attributes: &'static [wgpu::VertexAttribute],
) -> wgpu::VertexBufferLayout<'static> {
    assert!(
        attrs_fit(attributes, stride),
        "an attribute overruns its field"
    );
    wgpu::VertexBufferLayout {
        array_stride: stride as u64,
        step_mode,
        attributes,
    }
}

/// Whether `attrs`, in ascending offset, each read inside a vertex of `stride` bytes and stop
/// short of the next: the format of each fits its field.
const fn attrs_fit(attrs: &[wgpu::VertexAttribute], stride: usize) -> bool {
    let mut i = 0;
    while i < attrs.len() {
        let end = attrs[i].offset + attrs[i].format.size();
        let bound = if i + 1 < attrs.len() {
            attrs[i + 1].offset
        } else {
            stride as u64
        };
        if end > bound {
            return false;
        }
        i += 1;
    }
    true
}
