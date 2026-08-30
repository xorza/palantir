//! Every GPU texture the image pipeline samples, and the lifecycle each
//! kind rides.
//!
//! Two populations share one bind-group map: registered user images,
//! uploaded from [`ImageRegistry`] and freed when the owning
//! [`ImageHandle`](crate::ImageHandle) drops, and the framework-owned
//! off-screen targets a [`GpuView`](crate::widgets::gpu_view::GpuView)
//! paints into. One map because
//! [`ImagePipeline::draw`](crate::renderer::backend::image_pipeline::ImagePipeline::draw)
//! binds them identically and must find either in a single probe, and
//! because [`TextureIdSource`](crate::renderer::texture_id_source::TextureIdSource)
//! mints both so an id cannot mean two things.
//!
//! One type because every target operation ends in a binding: allocating
//! one inserts, resizing replaces, evicting removes. Splitting the two
//! would hand the target store a `&mut` on the binding store and write
//! the same insert/remove at four call sites.

mod render_target;

use crate::common::tracy;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_textures::render_target::{
    AllocatedTarget, RenderTarget, keep_target,
};
use crate::renderer::backend::texture_binding;
use crate::renderer::backend::texture_region::TextureRegion;
use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::renderer::gpu_paint::gpu_init_ctx::GpuInitCtx;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::image::FrameViews;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::text::shaper::TextShaper;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
pub(super) struct ImageTextures {
    /// `id → bind group` for every texture a draw may sample, of either
    /// population. An entry is inserted when the registry drains a
    /// pending upload or a target is allocated, and removed when the
    /// owning handle drops or the target is evicted. A draw for an
    /// absent id is skipped.
    bindings: FxHashMap<TextureId, wgpu::BindGroup>,
    /// The off-screen targets themselves, whose bind groups are the
    /// second population of `bindings`.
    targets: FxHashMap<TextureId, RenderTarget>,
    /// Group 0 layout (per-image texture + sampler). Built once; every
    /// bind group in `bindings` references it, and
    /// `ImagePipeline::build_variants` composes each format's pipeline
    /// layout against it — the only consumer outside this module.
    bgl: wgpu::BindGroupLayout,
    /// Shared by every image and `GpuView` target: min/mag nearest
    /// filtering is a shader-side UV texel-center snap, so all filter
    /// combinations ride one sampler and one bind group.
    sampler: wgpu::Sampler,
}

impl ImageTextures {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            bindings: FxHashMap::default(),
            targets: FxHashMap::default(),
            bgl: texture_binding::layout(device, "palantir.image.tex.bgl"),
            sampler: texture_binding::sampler(device, "palantir.image.sampler"),
        }
    }

    /// The group 0 layout every bind group here is built against, which
    /// each format's image pipeline layout composes over.
    pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }

    /// What a draw binds for `id`, or `None` when no texture of either
    /// population answers to it.
    pub(super) fn bind_group(&self, id: TextureId) -> Option<&wgpu::BindGroup> {
        self.bindings.get(&id)
    }

    /// Reconcile the GPU texture cache with the registry, once per frame
    /// from `WgpuBackend::submit` before the render pass. Uploads newly
    /// registered images (dropping each `Image` right after upload, so the
    /// CPU bytes don't outlive the GPU copy), then frees textures whose
    /// owning [`ImageHandle`](crate::ImageHandle) dropped. After this,
    /// every still-owned image has a bind group in the cache; a draw for
    /// any other id is silently skipped.
    ///
    /// Uploads run *before* drop-frees so an image registered and dropped
    /// in the same frame uploads then frees (no orphan) rather than
    /// free-then-upload (which would leak it into the cache un-owned).
    pub(super) fn drain_registry(&mut self, ctx: &mut GpuCtx<'_>, images: &ImageRegistry) {
        // Destructured so the upload borrows `bgl`/`sampler` while the
        // closure holds `bindings` mutably — disjoint fields, which
        // `self.upload(..)` inside the closure could not express.
        let Self {
            bindings,
            bgl,
            sampler,
            ..
        } = self;
        images.drain_pending(|id, image| {
            let bind_group = upload(ctx.device, ctx.queue, bgl, sampler, id, &image);
            bindings.insert(id, bind_group);
        });
        images.drain_dropped(|id| {
            bindings.remove(&id);
        });
    }

    /// Paint every [`GpuView`](crate::widgets::gpu_view::GpuView) drawn
    /// this frame into its off-screen target, before the main pass.
    /// Called once per frame from `WgpuBackend::submit`'s upload phase.
    /// This allocates or resizes each entry, registers its bind group,
    /// and runs [`GpuPaint::init`](crate::renderer::gpu_paint::GpuPaint::init)
    /// once, then `GpuPaint::paint` into it. Never touches the instance
    /// buffer, so it only has to run before the main pass samples the
    /// targets.
    ///
    /// Eviction is **immediate but owner-scoped**, and keyed on
    /// [`FrameViews::live`] — every view `owner` recorded this frame —
    /// rather than on [`FrameViews::draws`], which lists only the ones
    /// that painted. A view marked
    /// [`repaint(false)`](crate::widgets::gpu_view::GpuView::repaint) is
    /// culled out of the second and stays in the first, so it keeps its
    /// texture and `GpuPaint::init` is not re-run. A target is freed when
    /// its widget stops being recorded, which is the same sweep every
    /// other per-widget cache rides.
    ///
    /// `owner` is the submitting window's stable render-stream identity: the
    /// one shared backend serves all windows, so a submit may only evict its
    /// *own* dropped targets — another window's targets survive both this
    /// submit and their owner's idle (non-submitting) frames.
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
        self.retain_targets(|id, target| {
            keep_target(target.owner, owner, live.binary_search(id).is_ok())
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
        self.retain_targets(|_, target| target.owner != owner);
    }

    /// Keep the targets `keep` accepts and free the rest, dropping each
    /// dropped target's bind group with it. The one place a target and
    /// its binding part company, so neither eviction rule can leave the
    /// other population's entry behind.
    fn retain_targets(&mut self, mut keep: impl FnMut(&TextureId, &RenderTarget) -> bool) {
        let bindings = &mut self.bindings;
        self.targets.retain(|id, target| {
            let keep = keep(id, target);
            if !keep {
                bindings.remove(id);
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
    ) -> &mut RenderTarget {
        let Self {
            bindings,
            targets,
            bgl,
            sampler,
        } = self;
        match targets.entry(id) {
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                target.owner = owner;
                if target.size != size {
                    let allocated = AllocatedTarget::new(device, bgl, sampler, size);
                    target.view = allocated.view;
                    bindings.insert(id, allocated.bind_group);
                    target.size = size;
                }
                target
            }
            Entry::Vacant(entry) => {
                let allocated = AllocatedTarget::new(device, bgl, sampler, size);
                bindings.insert(id, allocated.bind_group);
                entry.insert(RenderTarget {
                    view: allocated.view,
                    size,
                    owner,
                    initialized: false,
                    last_paint: None,
                })
            }
        }
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    id: TextureId,
    image: &Image,
) -> wgpu::BindGroup {
    let raw_id = id.0;
    let size = wgpu::Extent3d {
        width: image.size.x,
        height: image.size.y,
        depth_or_array_layers: 1,
    };
    let texture_label = format!("palantir.image.tex.{raw_id:016x}");
    let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&texture_label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    TextureRegion {
        texture: &texture,
        first_row: 0,
        size: image.size,
        bytes_per_row: image.size.x * 4,
    }
    .write(queue, &image.pixels);
    let view = texture.create_view(&Default::default());
    texture_binding::bind_group(device, layout, sampler, &view, &bind_group_label)
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    //! Reach-in for the surface-format-change tests: GPU texture-cache
    //! occupancy, used to assert the cache survives a pipeline rebuild.

    use crate::renderer::backend::image_textures::ImageTextures;

    impl ImageTextures {
        /// Count of textures currently resident in the GPU cache.
        /// Lets the surface-format-change tests assert the cache survives
        /// a pipeline rebuild (surgical rebuild keeps it; a full rebuild
        /// would drop it to zero).
        pub(crate) fn gpu_cached_count(&self) -> usize {
            self.bindings.len()
        }
    }
}
