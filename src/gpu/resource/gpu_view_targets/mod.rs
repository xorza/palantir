//! The off-screen targets a [`GpuView`](crate::widgets::gpu_view::GpuView)
//! paints into, and the bind groups a draw samples them through.
//!
//! Registered images are the other population a draw can sample. Those
//! live in [`WgpuImageStore`](crate::gpu::resource::wgpu_image_store::WgpuImageStore),
//! and the two build against one [`ImageBinding`], so a composite of a
//! view binds exactly like an image. [`TextureId::reserve`](crate::primitives::identity::texture_id::TextureId::reserve)
//! mints both populations' ids, so an id cannot mean two things.

mod view_target;

use crate::common::tracy;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::device::gpu_init_ctx::GpuInitCtx;
use crate::gpu::frame::debug_marker;
use crate::gpu::resource::gpu_view_targets::view_target::{AllocatedTarget, ViewTarget};
use crate::gpu::resource::image_binding::ImageBinding;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::render_buffer::image::FrameViews;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::text::shaper::TextShaper;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
pub(crate) struct GpuViewTargets {
    /// One entry per view a live render stream still records. Inserted on
    /// the first paint, replaced on a resize, removed by the per-submit
    /// eviction or [`Self::retire_owner`].
    targets: FxHashMap<TextureId, ViewTarget>,
    binding: ImageBinding,
}

impl GpuViewTargets {
    pub(crate) fn new(binding: ImageBinding) -> Self {
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
        owner: RenderOwnerId,
        now: Duration,
        text: &TextShaper,
    ) {
        tracy::zone!();
        let FrameViews {
            draws,
            live,
            display_scale,
        } = views;
        // `live` arrives sorted — see `Frontend::build`.
        debug_assert!(
            draws
                .iter()
                .all(|draw| live.binary_search(&draw.id).is_ok()),
            "a painted GpuView target is missing from the frame's live roster",
        );
        for draw in draws {
            let target = self.ensure(ctx.device, draw.id, draw.used, owner);
            // A view the frame composites again without asking it to
            // repaint — damage that crosses a `repaint(false)` view —
            // still holds the pixels it was painted with.
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
                size_px: draw.used,
                full_px: draw.full,
                offset_px: draw.offset,
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

    /// Drop every target belonging to a render stream that will never submit
    /// again, freeing its textures and bind groups.
    ///
    /// [`keep_target`](crate::gpu::resource::gpu_view_targets::view_target::keep_target)
    /// preserves foreign owners' entries on every submit, so
    /// a closed window's targets would otherwise be held by the surviving
    /// windows for the life of the host.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "the winit host is the only caller today")
    )]
    pub(crate) fn retire_owner(&mut self, owner: RenderOwnerId) {
        self.targets.retain(|_, target| target.owner != owner);
    }

    fn ensure(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        size: UVec2,
        owner: RenderOwnerId,
    ) -> &mut ViewTarget {
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
mod tests {
    use crate::gpu::device::gpu_ctx::GpuCtx;
    use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
    use crate::gpu::resource::gpu_view_targets::GpuViewTargets;
    use crate::gpu::resource::image_binding::ImageBinding;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::primitives::identity::texture_id::TextureId;
    use crate::renderer::gpu_paint::GpuPaint;
    use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
    use crate::renderer::render_buffer::image::{FrameViews, RenderTargetDraw};
    use crate::renderer::render_owner_id::RenderOwnerId;
    use crate::text::shaper::TextShaper;
    use glam::UVec2;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::slice;
    use std::time::Duration;
    use wgpu::util::StagingBelt;

    #[derive(Debug)]
    struct CountingPaint(Rc<Cell<u32>>);

    impl GpuPaint for CountingPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// A target painted for a draw's stamp is composited again without a
    /// paint; a moved epoch, size, offset or scale paints. Each row is one
    /// submit and whether it ran the callback.
    #[test]
    fn an_unchanged_stamp_skips_the_paint() {
        let gpu = headless_test_gpu();
        let device = &gpu.device;
        let mut targets = GpuViewTargets::new(ImageBinding::new(device));
        let paints = Rc::new(Cell::new(0));
        let paint = GpuPaintRef(Rc::new(RefCell::new(CountingPaint(Rc::clone(&paints)))));
        let id = TextureId::reserve();
        let owner = RenderOwnerId::reserve();
        let shaper = TextShaper::new();
        let draw = RenderTargetDraw {
            id,
            used: UVec2::new(32, 24),
            full: UVec2::new(32, 24),
            offset: UVec2::ZERO,
            raster_scale: 1.0,
            paint,
            epoch: 1,
        };
        let submits = [
            (draw.clone(), true),
            (draw.clone(), false),
            (
                RenderTargetDraw {
                    epoch: 2,
                    ..draw.clone()
                },
                true,
            ),
            (
                RenderTargetDraw {
                    epoch: 2,
                    ..draw.clone()
                },
                false,
            ),
            (
                RenderTargetDraw {
                    epoch: 2,
                    used: UVec2::new(16, 24),
                    offset: UVec2::new(16, 0),
                    ..draw.clone()
                },
                true,
            ),
            (
                RenderTargetDraw {
                    epoch: 2,
                    used: UVec2::new(16, 24),
                    offset: UVec2::new(16, 0),
                    raster_scale: 2.0,
                    ..draw.clone()
                },
                true,
            ),
        ];
        let mut belt = StagingBelt::new(device.clone(), 1 << 12);
        for (at, (draw, painted)) in submits.into_iter().enumerate() {
            let before = paints.get();
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let mut ctx = GpuCtx::new(device, &gpu.queue, &mut belt, &mut encoder);
            targets.paint_gpu_views(
                &mut ctx,
                FrameViews {
                    draws: slice::from_ref(&draw),
                    live: &[id],
                    display_scale: 1.0,
                },
                owner,
                Duration::from_millis(16 * at as u64),
                &shaper,
            );
            assert_eq!(paints.get() - before, u32::from(painted), "submit {at}");
        }
    }
}
