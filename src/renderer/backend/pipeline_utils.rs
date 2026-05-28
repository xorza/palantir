//! Render-pipeline + bind-group-layout construction recipes shared by
//! the four pipeline modules so they can't drift on descriptor flags.
//! The dynamic-buffer abstraction lives in `super::dynamic_buffer`.

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
pub(super) struct PipelineRecipe<'a> {
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
pub(super) fn build_pipeline(device: &wgpu::Device, r: PipelineRecipe<'_>) -> wgpu::RenderPipeline {
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

/// Build a group-0 bind-group layout pairing a filterable 2D float
/// texture at binding 0 with a filtering sampler at binding 1, both
/// fragment-visible. The shape shared by the gradient LUT atlas
/// (`GradientResources`) and the per-image bind group (`ImagePipeline`).
pub(super) fn texture_sampler_bgl(
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
/// immediate-region size ([`super::IMMEDIATES_BYTES`]) so the
/// immediate state set by the backend at pass open (viewport) stays
/// valid as pipelines switch, and the text pipeline can additionally
/// write its `Params` at offset 8.
pub(super) fn build_pipeline_layout(
    device: &wgpu::Device,
    label: &'static str,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts,
        immediate_size: super::IMMEDIATES_BYTES,
    })
}
