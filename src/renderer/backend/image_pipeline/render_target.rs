//! The `GpuView` GPU executor for [`ImagePipeline`](super::ImagePipeline):
//! [`GpuViewTargets`] owns the framework's off-screen target textures (one
//! [`RenderTarget`] per live view, keyed by its stable [`TextureId`]) and
//! paints the app's `GpuPaint` callbacks into them each frame. Each target is
//! sized **exactly** to the view's used physical size, decided upstream on the
//! CPU and arriving per frame in `RenderBuffer.frame_targets` (which also
//! carries the paint callback); this file allocates to that size (reallocating
//! whenever it changes) and runs the paint. The composite samples the result
//! through `ImagePipeline`'s shared bind-group cache, lent to
//! [`GpuViewTargets::paint`].
//!
//! There is no `Ui`-side registry and so no explicit drop signal: a target not
//! drawn this frame is freed immediately — `frame_targets` is the exact live
//! set, so a view whose widget vanished (or that was culled off-screen) drops
//! its texture at once. The trade is a re-`init` + realloc if such a view
//! returns; a `GpuView` re-renders every frame, so an on-screen one is always
//! present and never thrashes.

use super::texture_bind_group;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::gpu_view::{GPU_VIEW_FORMAT, GpuFrameCtx, GpuInitCtx};
use crate::renderer::render_buffer::RenderTargetDraw;
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

/// All per-`GpuView` backend state, in one entry so a reallocation that
/// rewrites `view` + `size` preserves the rest. The color target is
/// `RENDER_ATTACHMENT` (the user's pass draws into it) + `TEXTURE_BINDING`
/// (the main pass samples it). Only the `view` is kept — it holds the
/// texture alive via wgpu's internal Arcs (same as `Backbuffer.stencil`),
/// and nothing here needs the `Texture` handle.
#[derive(Debug)]
pub(crate) struct RenderTarget {
    view: wgpu::TextureView,
    /// Currently-allocated physical-px size (the used size the composer's
    /// `frame_target` last requested). [`GpuViewTargets::paint`] recreates the
    /// texture only when the requested size differs from this.
    size: UVec2,
    /// Whether [`GpuPaint::init`] has run. Set once and preserved across
    /// reallocations (the recreated texture shares the build-time format).
    initialized: bool,
    /// Frame time of the last paint, for the `dt` handed to
    /// `GpuPaint::paint`. Preserved across reallocations so a resize
    /// doesn't spike `dt`. `None` until the first paint.
    last_paint: Option<Duration>,
}

impl RenderTarget {
    /// Wrap an already-created `view` as a fresh, uninitialized target.
    /// [`make_target`] builds the view (+ bind group) first, then hands it
    /// here so the texture isn't created twice.
    fn new_with(view: wgpu::TextureView, size: UVec2) -> Self {
        Self {
            view,
            size,
            initialized: false,
            last_paint: None,
        }
    }

    /// Create the texture + its view at `size`. Used both at first sight and
    /// on a reallocation (which only swaps `view` + `size`).
    fn create_view(device: &wgpu::Device, size: UVec2) -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.gpu_view.target"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
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

/// `ImagePipeline`'s bind-group machinery, lent to [`GpuViewTargets::paint`]
/// so it can register each painted target as a sampleable bind group in the
/// **shared** image `cache` (the composite then samples a `GpuView` target
/// exactly like any image). Bundled so the paint signature stays lean.
#[derive(Debug)]
pub(crate) struct BindGroupSink<'a> {
    pub(crate) bgl: &'a wgpu::BindGroupLayout,
    pub(crate) sampler: &'a wgpu::Sampler,
    pub(crate) cache: &'a mut FxHashMap<TextureId, wgpu::BindGroup>,
}

/// The `GpuView` GPU executor: the framework-owned off-screen targets (one
/// [`RenderTarget`] per live view, keyed by [`TextureId`]) plus the per-frame
/// paint of the app's `GpuPaint` callbacks into them. Held by
/// [`ImagePipeline`](super::ImagePipeline), which lends [`Self::paint`] its
/// shared bind-group cache so a `GpuView` target composites like any image.
#[derive(Debug, Default)]
pub(crate) struct GpuViewTargets {
    targets: FxHashMap<TextureId, RenderTarget>,
}

impl GpuViewTargets {
    /// Paint every `GpuView` composited this frame into its off-screen target,
    /// driven by the composer's `frame_targets` (one entry per composited view,
    /// each carrying its `TextureId`, used size, and `GpuPaintRef`). For each:
    /// (re)allocate the texture when the `used` size changed (registering its
    /// bind group in `sink`'s shared cache so the main pass can sample it), run
    /// [`GpuPaint::init`] once, then [`GpuPaint::paint`] into the target on the
    /// main encoder. Finally evict every target not in `frame_targets` (freed
    /// immediately — `frame_targets` is the exact live set). Never touches the
    /// instance buffer, so it only has to run before the main pass samples the
    /// targets.
    pub(crate) fn paint(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        mut sink: BindGroupSink<'_>,
        scale: f32,
        now: Duration,
    ) {
        for draw in frame_targets {
            let rt = self.ensure(ctx.device, &mut sink, draw.id, draw.used);
            let mut paint = draw.paint.0.borrow_mut();
            // Run `init` once per view (not on a realloc: the recreated
            // texture shares the build-time format).
            if !rt.initialized {
                paint.init(&GpuInitCtx {
                    device: ctx.device,
                    target_format: GPU_VIEW_FORMAT,
                });
                rt.initialized = true;
            }
            // Time since this view last painted (ZERO on its first paint).
            let dt = rt
                .last_paint
                .map_or(Duration::ZERO, |last| now.saturating_sub(last));
            paint.paint(&mut GpuFrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                encoder: ctx.encoder,
                target: &rt.view,
                size_px: draw.used,
                scale,
                dt,
            });
            rt.last_paint = Some(now);
        }
        // Evict immediately: `frame_targets` is the exact set of views live this
        // frame, so any target missing from it (its widget vanished, or it was
        // culled off-screen / fully occluded) is freed now — texture + shared-
        // cache bind group together. A view that returns re-`init`s + reallocs.
        self.targets.retain(|id, _| {
            let keep = frame_targets.iter().any(|draw| draw.id == *id);
            if !keep {
                sink.cache.remove(id);
            }
            keep
        });
    }

    /// The view's off-screen target, in a single `entry` lookup. Reuses the
    /// existing texture unless the requested `size` changed; on a change (or
    /// first sight) builds a fresh texture + bind group via [`make_target`]. A
    /// realloc swaps only the texture, so `init` + last-paint state persist.
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        sink: &mut BindGroupSink<'_>,
        id: TextureId,
        size: UVec2,
    ) -> &mut RenderTarget {
        match self.targets.entry(id) {
            Entry::Occupied(e) => {
                let rt = e.into_mut();
                if rt.size != size {
                    rt.view = make_target(device, sink, id, size);
                    rt.size = size;
                }
                rt
            }
            Entry::Vacant(e) => e.insert(RenderTarget::new_with(
                make_target(device, sink, id, size),
                size,
            )),
        }
    }
}

/// Create a `GPU_VIEW_FORMAT` texture view at `size` and register its
/// sampleable bind group in the shared cache under `id`. Shared by
/// [`GpuViewTargets::ensure`]'s first-sight + realloc paths.
fn make_target(
    device: &wgpu::Device,
    sink: &mut BindGroupSink<'_>,
    id: TextureId,
    size: UVec2,
) -> wgpu::TextureView {
    let view = RenderTarget::create_view(device, size);
    let bg = texture_bind_group(
        device,
        sink.bgl,
        sink.sampler,
        &view,
        "palantir.gpu_view.tex.bg",
    );
    sink.cache.insert(id, bg);
    view
}
