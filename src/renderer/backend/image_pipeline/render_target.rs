//! `GpuView` off-screen targets for [`ImagePipeline`]: the [`RenderTarget`]
//! the user's pass draws into. Its size is decided upstream by the
//! composer (the √2 ladder lives on
//! [`GpuViewSizes`](crate::renderer::gpu_view::GpuViewSizes)) and arrives
//! per frame in the [`RenderTargetDraw`](crate::renderer::render_buffer::RenderTargetDraw)
//! list, so this file is just the backend value type the allocator manages.
//! The per-frame reconcile that drives it is an inherent `ImagePipeline`
//! method and lives beside the struct in `mod.rs` (inherent impls stay with
//! their struct); this is the value type it calls into, exposed `pub(crate)`.

use crate::renderer::gpu_view::GPU_VIEW_FORMAT;
use glam::UVec2;
use std::time::Duration;

/// All per-`GpuView` backend state, in one entry so a reallocation that
/// rewrites `view` + `capacity` preserves the rest. The color target is
/// `RENDER_ATTACHMENT` (the user's pass draws into it) + `TEXTURE_BINDING`
/// (the main pass samples it). Only the `view` is kept — it holds the
/// texture alive via wgpu's internal Arcs (same as `Backbuffer.stencil`),
/// and nothing here needs the `Texture` handle.
pub(crate) struct RenderTarget {
    pub(crate) view: wgpu::TextureView,
    /// Currently-allocated physical-px size (the capacity the composer last
    /// requested for this target). Reconcile recreates the texture only
    /// when the requested capacity differs from this.
    pub(crate) capacity: UVec2,
    /// Whether [`GpuPaint::init`] has run. Set once and preserved across
    /// reallocations (the recreated texture shares the build-time format).
    pub(crate) initialized: bool,
    /// Frame time of the last paint, for the `dt` handed to
    /// `GpuPaint::paint`. Preserved across reallocations so a resize
    /// doesn't spike `dt`. `None` until the first paint.
    pub(crate) last_paint: Option<Duration>,
}

impl RenderTarget {
    /// Wrap an already-created `view` as a fresh, uninitialized target.
    /// Reconcile makes the view first (to build the bind group), then
    /// hands it here so the texture isn't created twice.
    pub(crate) fn new_with(view: wgpu::TextureView, capacity: UVec2) -> Self {
        Self {
            view,
            capacity,
            initialized: false,
            last_paint: None,
        }
    }

    /// Create the texture + its view at `capacity`. Used both at first
    /// sight and on a reallocation (which only swaps `view` + `capacity`).
    pub(crate) fn create_view(device: &wgpu::Device, capacity: UVec2) -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.gpu_view.target"),
            size: wgpu::Extent3d {
                width: capacity.x,
                height: capacity.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GPU_VIEW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    }
}
