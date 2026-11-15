//! One framework-owned off-screen target for a composited `GpuView`.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::resource::gpu_view_targets::TARGET_FORMAT;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::texel::TargetSpec;
use crate::renderer::render_buffer::render_target_draw::ViewStamp;
use glam::UVec2;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct ViewTarget {
    /// The texture's halves, which a resize makes again.
    pub(super) texture: AllocatedTarget,
    pub(super) owner: RenderOwnerId,
    /// [`GpuPaint::init`](crate::GpuPaint::init) has run for this view; per entry, not per texture, so a resize keeps it.
    pub(super) initialized: bool,
    pub(super) last_paint: Option<Duration>,
    pub(super) painted: Option<ViewStamp>,
}

/// A target texture as the halves a [`ViewTarget`] keeps, its view and bind group, at its size; a
/// resize makes a new one.
#[derive(Debug)]
pub(super) struct AllocatedTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) size: UVec2,
}

impl AllocatedTarget {
    pub(super) fn new(gpu: &Gpu, binding: &TextureBinding, size: UVec2) -> Self {
        let texture = TargetSpec {
            label: "palantir.gpu_view.target",
            size,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        }
        .texture(gpu);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = binding.texture_group(gpu, &view, "palantir.gpu_view.tex.bg");
        Self {
            view,
            bind_group,
            size,
        }
    }
}
