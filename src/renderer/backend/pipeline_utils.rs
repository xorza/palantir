//! Render-pipeline + bind-group-layout construction recipes shared by
//! the four pipeline modules so they can't drift on descriptor flags.
//! The dynamic-buffer abstraction lives in `crate::renderer::backend::dynamic_buffer`.

use crate::renderer::backend::IMMEDIATES_BYTES;

/// Render-pipeline recipe. Threads the call-site fields each pipeline
/// genuinely varies (label, shader, layout, vertex buffers, topology,
/// color format, fragment entry, color writes, blend, optional
/// depth-stencil) and lets [`build_pipeline`] fill in the rest with
/// the project-wide defaults (single color target, no MSAA, no
/// multiview, vertex entry = `"vs"`).
///
/// `'a` is the lifetime of the references passed in; the returned
/// [`wgpu::RenderPipeline`] retains its own internal references and
/// outlives the recipe.
pub(crate) struct PipelineRecipe<'a> {
    pub label: &'static str,
    pub shader: &'a wgpu::ShaderModule,
    pub layout: &'a wgpu::PipelineLayout,
    pub vertex_buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub topology: wgpu::PrimitiveTopology,
    pub color_format: wgpu::TextureFormat,
    pub fragment_entry: &'static str,
    pub color_writes: wgpu::ColorWrites,
    pub blend: Option<wgpu::BlendState>,
    pub depth_stencil: Option<wgpu::DepthStencilState>,
}

/// Build a render pipeline from a [`PipelineRecipe`]. Sole source of
/// truth for the descriptor fields each pipeline doesn't vary —
/// vertex entry, sample count, multiview mask. The mesh / quad /
/// image pipelines + their lazy stencil variants all go through here.
pub(crate) fn build_pipeline(device: &wgpu::Device, r: PipelineRecipe<'_>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(r.label),
        layout: Some(r.layout),
        vertex: wgpu::VertexState {
            module: r.shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: r.vertex_buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: r.shader,
            entry_point: Some(r.fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: r.color_format,
                blend: r.blend,
                write_mask: r.color_writes,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: r.topology,
            ..Default::default()
        },
        depth_stencil: r.depth_stencil,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// A color render pipeline paired with its stencil-test twin (the same
/// recipe plus [`crate::renderer::backend::stencil::stencil_test_state`]).
/// `base` runs on plain frames; `test` runs in the stencil-attached
/// rounded-clip pass. Shared by the quad / mesh / image / curve
/// pipelines so base-vs-test selection can't drift across them. Both
/// are built up front so a
/// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
/// set is complete the moment it exists.
#[derive(Debug)]
pub(crate) struct StencilVariant {
    base: wgpu::RenderPipeline,
    test: wgpu::RenderPipeline,
}

impl StencilVariant {
    pub(crate) fn new(base: wgpu::RenderPipeline, test: wgpu::RenderPipeline) -> Self {
        Self { base, test }
    }

    /// The pipeline to bind: the stencil-test twin in a rounded-clip
    /// pass, otherwise the base.
    pub(crate) fn select(&self, use_stencil: bool) -> &wgpu::RenderPipeline {
        if use_stencil { &self.test } else { &self.base }
    }
}

/// Build a group-0 bind-group layout pairing a filterable 2D float
/// texture at binding 0 with a filtering sampler at binding 1, both
/// fragment-visible. The shape shared by the gradient LUT atlas
/// (`GradientResources`) and the per-image bind group (`ImagePipeline`).
pub(crate) fn texture_sampler_bgl(
    device: &wgpu::Device,
    label: &'static str,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Build a pipeline layout. Every palantir pipeline declares the same
/// immediate-region size ([`crate::renderer::backend::IMMEDIATES_BYTES`]) so the
/// immediate state set by the backend at pass open (viewport) stays
/// valid as pipelines switch, and the text pipeline can additionally
/// write its `Params` at offset 8.
pub(crate) fn build_pipeline_layout(
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
