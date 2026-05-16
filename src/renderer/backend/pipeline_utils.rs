//! Small shared helpers for the wgpu pipelines (quad / mesh / image).
//! Owns the regrow-instance-buffer + render-pipeline construction
//! recipes so the three pipelines don't drift on layout / state /
//! descriptor flags.

/// Grow `buffer` to fit `needed_len` items of `item_size` bytes,
/// rounding up to the next power of two (floored at `min_capacity`).
/// `capacity` tracks the slot count, not bytes. No-op when `needed_len
/// <= *capacity`. Single source of truth for the wgpu vertex/index/
/// instance buffer regrow pattern — see `quad_pipeline.rs`,
/// `mesh_pipeline.rs`, `image_pipeline.rs`.
#[allow(clippy::too_many_arguments)]
pub(super) fn grow_instance_buffer(
    device: &wgpu::Device,
    buffer: &mut wgpu::Buffer,
    capacity: &mut usize,
    needed_len: usize,
    item_size: usize,
    usage: wgpu::BufferUsages,
    label: &'static str,
    min_capacity: usize,
) {
    if needed_len <= *capacity {
        return;
    }
    *capacity = needed_len.next_power_of_two().max(min_capacity);
    *buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (*capacity * item_size) as u64,
        usage,
        mapped_at_creation: false,
    });
}

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

/// Build a pipeline layout. Trivial but kills the three-way repeat of
/// `PipelineLayoutDescriptor { label, bind_group_layouts, immediate_size: 0 }`.
pub(super) fn build_pipeline_layout(
    device: &wgpu::Device,
    label: &'static str,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts,
        immediate_size: 0,
    })
}
