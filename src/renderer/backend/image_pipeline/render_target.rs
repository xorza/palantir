//! Framework-owned off-screen targets for composited `GpuView`s.

use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::textures::ImageTextures;
use crate::renderer::gpu_view::{GpuFrameCtx, GpuInitCtx};
use crate::renderer::render_buffer::image::RenderTargetDraw;
use crate::renderer::render_owner::RenderOwnerId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug, Default)]
pub(super) struct GpuViewTargets {
    entries: FxHashMap<TextureId, RenderTarget>,
    submit_epoch: u64,
}

impl GpuViewTargets {
    #[profiling::function]
    pub(super) fn paint(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        owner: RenderOwnerId,
        now: Duration,
        textures: &mut ImageTextures,
    ) {
        self.submit_epoch = self
            .submit_epoch
            .checked_add(1)
            .expect("GpuView target submit epoch overflowed");
        let submit_epoch = self.submit_epoch;
        for draw in frame_targets {
            let target = self.ensure(
                ctx.device,
                draw.id,
                draw.used,
                owner,
                submit_epoch,
                textures,
            );
            let mut paint = draw.paint.0.borrow_mut();
            if !target.initialized {
                profiling::scope!("GpuView::init");
                debug_marker::push_encoder(ctx.encoder, "palantir.gpu_view.init");
                paint.init(&GpuInitCtx {
                    device: ctx.device,
                    target_format: TARGET_FORMAT,
                });
                debug_marker::pop_encoder(ctx.encoder);
                target.initialized = true;
            }
            let dt = target
                .last_paint
                .map_or(Duration::ZERO, |last| now.saturating_sub(last));
            profiling::scope!("GpuView::paint");
            debug_marker::push_encoder(ctx.encoder, "palantir.gpu_view.paint");
            paint.paint(&mut GpuFrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                encoder: ctx.encoder,
                target: &target.view,
                size_px: draw.used,
                display_scale: draw.display_scale,
                raster_scale: draw.raster_scale,
                dt,
            });
            debug_marker::pop_encoder(ctx.encoder);
            target.last_paint = Some(now);
        }
        self.entries.retain(|id, target| {
            let keep = keep_target(target.owner, target.submit_epoch, owner, submit_epoch);
            if !keep {
                textures.bindings.remove(id);
            }
            keep
        });
    }

    /// Drop every target belonging to a render stream that will never submit
    /// again, freeing its textures and bind groups.
    ///
    /// [`keep_target`] preserves foreign owners' entries on every submit, so
    /// a closed window's targets would otherwise be held by the surviving
    /// windows for the life of the host.
    #[cfg_attr(not(feature = "winit-host"), allow(dead_code))]
    pub(super) fn retire_owner(&mut self, owner: RenderOwnerId, textures: &mut ImageTextures) {
        self.entries.retain(|id, target| {
            let keep = target.owner != owner;
            if !keep {
                textures.bindings.remove(id);
            }
            keep
        });
    }

    fn ensure(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        size: UVec2,
        owner: RenderOwnerId,
        submit_epoch: u64,
        textures: &mut ImageTextures,
    ) -> &mut RenderTarget {
        match self.entries.entry(id) {
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                target.owner = owner;
                target.submit_epoch = submit_epoch;
                if target.size != size {
                    let allocated = allocate(device, textures, size);
                    target.view = allocated.view;
                    textures.bindings.insert(id, allocated.bind_group);
                    target.size = size;
                }
                target
            }
            Entry::Vacant(entry) => {
                let allocated = allocate(device, textures, size);
                textures.bindings.insert(id, allocated.bind_group);
                entry.insert(RenderTarget {
                    view: allocated.view,
                    size,
                    owner,
                    submit_epoch,
                    initialized: false,
                    last_paint: None,
                })
            }
        }
    }
}

#[derive(Debug)]
struct RenderTarget {
    view: wgpu::TextureView,
    size: UVec2,
    owner: RenderOwnerId,
    submit_epoch: u64,
    initialized: bool,
    last_paint: Option<Duration>,
}

#[derive(Debug)]
struct AllocatedTarget {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

fn allocate(device: &wgpu::Device, textures: &ImageTextures, size: UVec2) -> AllocatedTarget {
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
    let bind_group = textures.bind_group(device, &view, "palantir.gpu_view.tex.bg");
    AllocatedTarget { view, bind_group }
}

/// Per-submit eviction: keep an entry unless the submitting owner skipped
/// it this frame. Another owner's entries always survive, since an idle
/// window is not evidence that its views are gone — which is exactly why a
/// *closed* owner has to be retired explicitly through
/// [`GpuViewTargets::retire_owner`], never having a submit to be absent from.
fn keep_target(
    entry_owner: RenderOwnerId,
    entry_submit_epoch: u64,
    owner: RenderOwnerId,
    submit_epoch: u64,
) -> bool {
    entry_owner != owner || entry_submit_epoch == submit_epoch
}

#[cfg(test)]
mod tests {
    use super::keep_target;
    use crate::renderer::render_owner::RenderOwnerId;

    fn evicted(
        entries: &[(u64, RenderOwnerId, u64)],
        owner: RenderOwnerId,
        submit_epoch: u64,
    ) -> Vec<u64> {
        entries
            .iter()
            .filter(|(_, entry_owner, entry_submit_epoch)| {
                !keep_target(*entry_owner, *entry_submit_epoch, owner, submit_epoch)
            })
            .map(|(id, _, _)| *id)
            .collect()
    }

    #[test]
    fn eviction_uses_current_submit_epoch_and_is_owner_scoped() {
        // Note `2` (owner `b`) surviving every one of `a`'s submits below:
        // a submit is never evidence about another stream's targets. That is
        // what leaves a *closed* stream's targets unreachable by eviction and
        // makes `GpuViewTargets::retire_owner` the only thing that frees them.
        let a = RenderOwnerId::reserve();
        let b = RenderOwnerId::reserve();
        let entries = [(1, a, 7), (3, a, 6), (2, b, 4)];
        let cases = [
            (a, 7, vec![3]),
            (a, 8, vec![1, 3]),
            (b, 4, vec![]),
            (b, 5, vec![2]),
        ];
        for (owner, submit_epoch, expected) in cases {
            assert_eq!(evicted(&entries, owner, submit_epoch), expected);
        }
    }
}
