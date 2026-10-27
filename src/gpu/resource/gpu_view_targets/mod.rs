//! The off-screen targets a [`GpuView`](crate::widgets::gpu_view::GpuView) paints into, and the bind groups a draw samples them through.
//!
//! Registered images live in [`WgpuImageStore`](crate::gpu::resource::wgpu_image_store::WgpuImageStore); both build against one [`TextureBinding`], and [`TextureId::reserve`](crate::primitives::identity::texture_id::TextureId::reserve) mints both id populations, so an id is unambiguous.

mod view_target;

use crate::common::tracy;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::device::gpu_init_ctx::GpuInitCtx;
use crate::gpu::frame::debug_marker;
use crate::gpu::resource::gpu_view_targets::view_target::{AllocatedTarget, ViewTarget};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::render_buffer::render_target_draw::{FrameViews, RenderTargetDraw};
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::text::shaper::TextShaper;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The render stream painting views this submit, its frame time, and the shaper a view's init
/// receives so its text agrees with the UI around it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewSubmitter<'a> {
    pub(crate) owner: RenderOwnerId,
    pub(crate) now: Duration,
    pub(crate) text: &'a TextShaper,
}

#[derive(Debug)]
pub(crate) struct GpuViewTargets {
    /// One entry per view a live render stream still records: inserted on first paint, replaced on resize, removed by per-submit eviction or [`Self::retire_owner`].
    targets: FxHashMap<TextureId, ViewTarget>,
    binding: TextureBinding,
}

impl GpuViewTargets {
    pub(crate) fn new(binding: TextureBinding) -> Self {
        Self {
            targets: FxHashMap::default(),
            binding,
        }
    }

    pub(crate) fn bind_group(&self, id: TextureId) -> Option<&wgpu::BindGroup> {
        self.targets.get(&id).map(|target| &target.bind_group)
    }

    pub(crate) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        views: FrameViews<'_>,
        submitter: ViewSubmitter<'_>,
    ) {
        tracy::zone!();
        let ViewSubmitter { owner, now, text } = submitter;
        let FrameViews {
            draws,
            live,
            display_scale,
        } = views;
        // `live` arrives sorted; see `Frontend::build`.
        debug_assert!(
            draws
                .iter()
                .all(|draw| live.binary_search(&draw.id).is_ok()),
            "a painted GpuView target is missing from the frame's live roster",
        );
        for draw in draws {
            let target = self.ensure(ctx.device, draw, owner);
            // A view composited again without a repaint (damage crossing a `repaint(false)` view) keeps its painted pixels.
            let stamp = draw.stamp(display_scale);
            if target.painted == Some(stamp) {
                continue;
            }
            let mut paint = draw.paint.0.borrow_mut();
            if !target.initialized {
                tracy::zone!("GpuView::init");
                debug_marker::push_encoder(ctx.encoder, "palantir.gpu_view.init");
                paint.init(&GpuInitCtx {
                    device: ctx.device,
                    target_format: TARGET_FORMAT,
                    text,
                });
                debug_marker::pop_encoder(ctx.encoder);
                target.initialized = true;
            }
            let dt = target
                .last_paint
                .map_or(Duration::ZERO, |last| now.saturating_sub(last));
            tracy::zone!("GpuView::paint");
            debug_marker::push_encoder(ctx.encoder, "palantir.gpu_view.paint");
            paint.paint(&mut GpuFrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                encoder: ctx.encoder,
                target: &target.view,
                physical_size: draw.used,
                physical_full_size: draw.full,
                physical_offset: draw.offset,
                display_scale,
                raster_scale: draw.raster_scale,
                dt,
            });
            debug_marker::pop_encoder(ctx.encoder);
            target.last_paint = Some(now);
            target.painted = Some(stamp);
        }
        self.targets.retain(|id, target| {
            view_target::keep_target(target.owner, owner, live.binary_search(id).is_ok())
        });
    }

    /// Drop every target of a render stream that will never submit again, freeing textures and bind groups. [`keep_target`](crate::gpu::resource::gpu_view_targets::view_target::keep_target) preserves foreign owners' entries each submit, so a closed window's targets would otherwise outlive it.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "the winit host is the only caller today")
    )]
    pub(crate) fn retire_owner(&mut self, owner: RenderOwnerId) {
        self.targets.retain(|_, target| target.owner != owner);
    }

    /// `draw`'s target, owned by `owner` and sized to its [`capacity`](RenderTargetDraw::capacity), allocated or resized as needed.
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        draw: &RenderTargetDraw,
        owner: RenderOwnerId,
    ) -> &mut ViewTarget {
        let size = draw.capacity();
        match self.targets.entry(draw.id) {
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                target.owner = owner;
                if target.size != size {
                    let allocated = AllocatedTarget::new(device, &self.binding, size);
                    target.view = allocated.view;
                    target.bind_group = allocated.bind_group;
                    target.size = size;
                }
                target
            }
            Entry::Vacant(entry) => {
                let allocated = AllocatedTarget::new(device, &self.binding, size);
                entry.insert(ViewTarget {
                    view: allocated.view,
                    bind_group: allocated.bind_group,
                    size,
                    owner,
                    initialized: false,
                    last_paint: None,
                    painted: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
