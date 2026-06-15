//! Off-screen `GpuView` target state + allocator, driven by
//! [`ImagePipeline::ensure_target`](super::ImagePipeline) /
//! [`ImagePipeline::paint_gpu_views`](super::ImagePipeline). The inherent
//! methods stay with the struct in `mod.rs`; the value type and the
//! `make_target` allocator they call live here.

use super::texture_bind_group;
use crate::renderer::gpu_view::GPU_VIEW_FORMAT;
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::time::Duration;

/// All per-`GpuView` off-screen-target state, in one entry so a reallocation
/// that rewrites `view` + `size` preserves the rest. The color target is
/// `RENDER_ATTACHMENT` (the user's pass draws into it) + `TEXTURE_BINDING` (the
/// main pass samples it). Only the `view` is kept — it holds the texture alive
/// via wgpu's internal Arcs (same as `Backbuffer.stencil`).
#[derive(Debug)]
pub(crate) struct RenderTarget {
    pub(crate) view: wgpu::TextureView,
    /// Currently-allocated physical-px size;
    /// [`ImagePipeline::ensure_target`](super::ImagePipeline) recreates the
    /// texture only when the requested size differs from this.
    pub(crate) size: UVec2,
    /// Whether `GpuPaint::init` has run. Set once and preserved across
    /// reallocations (the recreated texture shares the build-time format).
    pub(crate) initialized: bool,
    /// Frame time of the last paint, for the `dt` handed to `GpuPaint::paint`.
    /// Preserved across reallocations so a resize doesn't spike `dt`. `None`
    /// until the first paint.
    pub(crate) last_paint: Option<Duration>,
}

/// Create a `GPU_VIEW_FORMAT` off-screen texture view at `size` and register its
/// sampleable bind group in the shared `cache` under `id`. Shared by
/// [`ImagePipeline::ensure_target`](super::ImagePipeline)'s first-sight +
/// realloc paths.
pub(crate) fn make_target(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    cache: &mut FxHashMap<TextureId, wgpu::BindGroup>,
    id: TextureId,
    size: UVec2,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.gpu_view.target"),
        size: wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: GPU_VIEW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bg = texture_bind_group(device, bgl, sampler, &view, "palantir.gpu_view.tex.bg");
    cache.insert(id, bg);
    view
}
