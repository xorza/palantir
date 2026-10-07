//! One framework-owned off-screen target for a composited `GpuView`.

use crate::gpu::resource::gpu_view_targets::TARGET_FORMAT;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target;
use crate::renderer::render_buffer::image::ViewStamp;
use crate::renderer::render_owner_id::RenderOwnerId;
use glam::UVec2;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct ViewTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) size: UVec2,
    pub(super) owner: RenderOwnerId,
    /// [`GpuPaint::init`](crate::GpuPaint::init) has run for this view; per entry, not per texture, so a resize keeps it.
    pub(super) initialized: bool,
    pub(super) last_paint: Option<Duration>,
    pub(super) painted: Option<ViewStamp>,
}

/// A fresh target texture as the two halves a [`ViewTarget`] keeps (view and bind group); a resize swaps both.
#[derive(Debug)]
pub(super) struct AllocatedTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) bind_group: wgpu::BindGroup,
}

impl AllocatedTarget {
    pub(super) fn new(device: &wgpu::Device, binding: &TextureBinding, size: UVec2) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.gpu_view.target"),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = binding.bind_group(device, &view, "palantir.gpu_view.tex.bg");
        Self { view, bind_group }
    }
}

/// Per-submit eviction: keep an entry unless the submitting owner has stopped recording it (`live` means still
/// recorded, not painted this frame). Another owner's entries always survive, so a *closed* owner must be
/// retired explicitly through
/// [`GpuViewTargets::retire_owner`](super::GpuViewTargets::retire_owner),
/// (it never has a submit to be absent from).
pub(super) fn keep_target(entry_owner: RenderOwnerId, owner: RenderOwnerId, live: bool) -> bool {
    entry_owner != owner || live
}

#[cfg(test)]
mod tests {
    use super::keep_target;
    use crate::renderer::render_owner_id::RenderOwnerId;

    fn evicted(entries: &[(u64, RenderOwnerId)], owner: RenderOwnerId, live: &[u64]) -> Vec<u64> {
        entries
            .iter()
            .filter(|(id, entry_owner)| !keep_target(*entry_owner, owner, live.contains(id)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Eviction asks "does the submitter still record this view?", never "did it paint one this frame".
    /// Owner `b`'s `2` survives all of `a`'s submits, so only `retire_owner` frees a closed stream's targets.
    #[test]
    fn eviction_follows_the_live_roster_and_is_owner_scoped() {
        let a = RenderOwnerId::reserve();
        let b = RenderOwnerId::reserve();
        let entries = [(1, a), (3, a), (2, b)];
        let cases = [
            (a, &[1u64, 3][..], vec![]),
            (a, &[1][..], vec![3]),
            (a, &[][..], vec![1, 3]),
            (b, &[2][..], vec![]),
            (b, &[][..], vec![2]),
        ];
        for (owner, live, expected) in cases {
            assert_eq!(evicted(&entries, owner, live), expected, "live={live:?}");
        }
    }
}
