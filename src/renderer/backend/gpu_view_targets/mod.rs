//! The off-screen targets a [`GpuView`](crate::widgets::gpu_view::GpuView)
//! paints into, and the bind groups a draw samples them through.
//!
//! Registered images are the other population a draw can sample. Those
//! live in [`WgpuImageStore`](crate::renderer::backend::image_store::WgpuImageStore),
//! and the two build against one [`ImageBinding`], so a composite of a
//! view binds exactly like an image. [`TextureIdSource`](crate::renderer::texture_id_source::TextureIdSource)
//! mints both populations' ids, so an id cannot mean two things.

mod render_target;

use crate::common::tracy;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::gpu_view_targets::render_target::{AllocatedTarget, RenderTarget};
use crate::renderer::backend::image_binding::ImageBinding;
use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::renderer::gpu_paint::gpu_init_ctx::GpuInitCtx;
use crate::renderer::render_buffer::image::FrameViews;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::text::shaper::TextShaper;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
pub(super) struct GpuViewTargets {
    /// One entry per view a live render stream still records. Inserted on
    /// the first paint, replaced on a resize, removed by the per-submit
    /// eviction or [`Self::retire_owner`].
    targets: FxHashMap<TextureId, RenderTarget>,
    binding: ImageBinding,
}

impl GpuViewTargets {
    pub(super) fn new(binding: ImageBinding) -> Self {
        Self {
            targets: FxHashMap::default(),
            binding,
        }
    }

    pub(super) fn bind_group(&self, id: TextureId) -> Option<&wgpu::BindGroup> {
        self.targets.get(&id).map(|target| &target.bind_group)
    }

    pub(super) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        views: FrameViews<'_>,
        owner: RenderOwnerId,
        now: Duration,
        text: &TextShaper,
    ) {
        tracy::zone!();
        let FrameViews { draws, live } = views;
        // `live` arrives sorted — see `Frontend::build`.
        debug_assert!(
            draws
                .iter()
                .all(|draw| live.binary_search(&draw.id).is_ok()),
            "a painted GpuView target is missing from the frame's live roster",
        );
        for draw in draws {
            let target = self.ensure(ctx.device, draw.id, draw.used, owner);
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
                size_px: draw.used,
                full_px: draw.full,
                offset_px: draw.offset,
                display_scale: draw.display_scale,
                raster_scale: draw.raster_scale,
                dt,
            });
            debug_marker::pop_encoder(ctx.encoder);
            target.last_paint = Some(now);
        }
        self.targets.retain(|id, target| {
            render_target::keep_target(target.owner, owner, live.binary_search(id).is_ok())
        });
    }

    /// Drop every target belonging to a render stream that will never submit
    /// again, freeing its textures and bind groups.
    ///
    /// [`keep_target`] preserves foreign owners' entries on every submit, so
    /// a closed window's targets would otherwise be held by the surviving
    /// windows for the life of the host.
    #[cfg_attr(not(feature = "winit"), allow(dead_code))]
    pub(super) fn retire_owner(&mut self, owner: RenderOwnerId) {
        self.targets.retain(|_, target| target.owner != owner);
    }

    fn ensure(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        size: UVec2,
        owner: RenderOwnerId,
    ) -> &mut RenderTarget {
        match self.targets.entry(id) {
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
                entry.insert(RenderTarget {
                    view: allocated.view,
                    bind_group: allocated.bind_group,
                    size,
                    owner,
                    initialized: false,
                    last_paint: None,
                })
            }
        }
    }
}
