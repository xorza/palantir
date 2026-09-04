//! The binding shape every image draw samples through: the group-0
//! layout and the sampler, built once and shared by the registered images
//! and the `GpuView` targets.

use crate::renderer::backend::texture_binding;

/// The per-image group-0 layout and the sampler it pairs with.
///
/// `Clone` hands out `wgpu`'s own reference-counted handles, so the
/// registry's GPU side and the target store hold one layout between them,
/// and each format's image pipeline composes over that same one.
#[derive(Clone, Debug)]
pub(super) struct ImageBinding {
    layout: wgpu::BindGroupLayout,
    /// Shared by every image and `GpuView` target: min/mag nearest
    /// filtering is a shader-side UV texel-centre snap, so all filter
    /// combinations ride one sampler and one bind group.
    sampler: wgpu::Sampler,
}

impl ImageBinding {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            layout: texture_binding::layout(device, "palantir.image.tex.bgl"),
            sampler: texture_binding::sampler(device, "palantir.image.sampler"),
        }
    }

    pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    pub(super) fn bind_group(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::BindGroup {
        texture_binding::bind_group(device, &self.layout, &self.sampler, view, label)
    }
}
