//! One framework-owned off-screen target for a composited `GpuView`.

use crate::renderer::backend::image_textures::TARGET_FORMAT;
use crate::renderer::backend::texture_binding;
use crate::renderer::render_owner_id::RenderOwnerId;
use glam::UVec2;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct RenderTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) size: UVec2,
    pub(super) owner: RenderOwnerId,
    /// `GpuPaint::init` has run against this texture. Survives every frame
    /// the view is merely undamaged, because the texture does — see
    /// [`ImageTextures::paint_gpu_views`](super::ImageTextures::paint_gpu_views).
    pub(super) initialized: bool,
    pub(super) last_paint: Option<Duration>,
}

/// A freshly created target texture, as the two halves its owner files
/// separately: the view a paint renders into, and the bind group a draw
/// samples it through.
#[derive(Debug)]
pub(super) struct AllocatedTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) bind_group: wgpu::BindGroup,
}

impl AllocatedTarget {
    pub(super) fn new(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: UVec2,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.gpu_view.target"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group =
            texture_binding::bind_group(device, bgl, sampler, &view, "palantir.gpu_view.tex.bg");
        Self { view, bind_group }
    }
}

/// Per-submit eviction: keep an entry unless the submitting owner has
/// stopped recording it. `live` says the submitter still has the view — not
/// that it painted one this frame, which is what makes an undamaged view
/// free to sit a frame out.
///
/// Another owner's entries always survive, since an idle window is not
/// evidence that its views are gone — which is exactly why a *closed* owner
/// has to be retired explicitly through
/// [`ImageTextures::retire_owner`](super::ImageTextures::retire_owner),
/// never having a submit to be absent from.
///
/// A free function over the two ids rather than a method on
/// [`RenderTarget`], because a target owns a `wgpu::TextureView` and the
/// rule is worth a table test that no device has to exist for.
pub(super) fn keep_target(entry_owner: RenderOwnerId, owner: RenderOwnerId, live: bool) -> bool {
    entry_owner != owner || live
}

#[cfg(test)]
mod tests {
    use super::keep_target;
    use crate::renderer::render_owner_id::RenderOwnerId;

    /// Which of `entries` (id, owner) a submit by `owner` frees, given the
    /// ids that submit still lists as live.
    fn evicted(entries: &[(u64, RenderOwnerId)], owner: RenderOwnerId, live: &[u64]) -> Vec<u64> {
        entries
            .iter()
            .filter(|(id, entry_owner)| !keep_target(*entry_owner, owner, live.contains(id)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Eviction asks "does the submitter still record this view?", never
    /// "did it paint one this frame" — so a view that skipped its paint is
    /// indistinguishable from one that painted, and only a view the app
    /// stopped recording is freed.
    ///
    /// Note `2` (owner `b`) surviving every one of `a`'s submits below: a
    /// submit is never evidence about another stream's targets. That is what
    /// leaves a *closed* stream's targets unreachable by eviction and makes
    /// `ImageTextures::retire_owner` the only thing that frees them.
    #[test]
    fn eviction_follows_the_live_roster_and_is_owner_scoped() {
        let a = RenderOwnerId::reserve();
        let b = RenderOwnerId::reserve();
        let entries = [(1, a), (3, a), (2, b)];
        let cases = [
            // `a` still records both of its views — nothing freed, whatever
            // subset of them actually painted this frame.
            (a, &[1u64, 3][..], vec![]),
            // `a` dropped view 3.
            (a, &[1][..], vec![3]),
            // `a` dropped both.
            (a, &[][..], vec![1, 3]),
            // `b` submitting frees nothing of `a`'s, and keeps its own.
            (b, &[2][..], vec![]),
            (b, &[][..], vec![2]),
        ];
        for (owner, live, expected) in cases {
            assert_eq!(evicted(&entries, owner, live), expected, "live={live:?}");
        }
    }
}
