//! The frame group: what every color pipeline of the main pass reads at one slot.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas;
use crate::gpu::resource::texture_binding::TextureBinding;

/// The group every color pipeline reads at
/// [`FRAME_GROUP`](crate::primitives::wgsl::FRAME_GROUP), which the main pass binds once: the
/// gradient atlas. Owned by the backend beside the atlas, made again when the atlas grows.
#[derive(Debug)]
pub(crate) struct FrameGroup {
    binding: TextureBinding,
    group: wgpu::BindGroup,
    /// The atlas generation `group` binds, which a grown atlas changes.
    atlas: u64,
}

impl FrameGroup {
    /// The group over `atlas`.
    pub(crate) fn new(gpu: &Gpu, binding: TextureBinding, atlas: &GpuGradientAtlas) -> Self {
        let group = binding.frame_group(gpu, atlas.view());
        Self {
            binding,
            group,
            atlas: atlas.generation(),
        }
    }

    /// Bind `atlas`, making the group again where the atlas grew.
    pub(crate) fn update(&mut self, gpu: &Gpu, atlas: &GpuGradientAtlas) {
        if atlas.generation() != self.atlas {
            self.group = self.binding.frame_group(gpu, atlas.view());
            self.atlas = atlas.generation();
        }
    }

    /// The group the main pass binds.
    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }
}
