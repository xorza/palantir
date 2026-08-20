//! Framework-owned off-screen targets for composited `GpuView`s.

use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::textures::ImageTextures;
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

#[derive(Debug, Default)]
pub(super) struct GpuViewTargets {
    entries: FxHashMap<TextureId, RenderTarget>,
}

impl GpuViewTargets {
    /// Paint every [`GpuView`](crate::widgets::gpu_view::GpuView) drawn
    /// this frame into its off-screen target, before the main pass.
    /// Called once per frame from `WgpuBackend::submit`'s upload phase,
    /// through `ImagePipeline::paint_gpu_views`. This store allocates or
    /// resizes each entry, registers its bind group in the shared
    /// image-texture store, and runs
    /// [`GpuPaint::init`](crate::renderer::gpu_paint::GpuPaint::init)
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
    #[profiling::function]
    pub(super) fn paint(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        views: FrameViews<'_>,
        owner: RenderOwnerId,
        now: Duration,
        textures: &mut ImageTextures,
        text: &TextShaper,
    ) {
        let FrameViews { draws, live } = views;
        debug_assert!(
            draws.iter().all(|draw| live.contains(&draw.id)),
            "a painted GpuView target is missing from the frame's live roster",
        );
        for draw in draws {
            let target = self.ensure(ctx.device, draw.id, draw.used, owner, textures);
            let mut paint = draw.paint.0.borrow_mut();
            if !target.initialized {
                profiling::scope!("GpuView::init");
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
            profiling::scope!("GpuView::paint");
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
        self.entries.retain(|id, target| {
            let keep = keep_target(target.owner, owner, live.contains(id));
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
        textures: &mut ImageTextures,
    ) -> &mut RenderTarget {
        match self.entries.entry(id) {
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                target.owner = owner;
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
    /// `GpuPaint::init` has run against this texture. Survives every frame
    /// the view is merely undamaged, because the texture does — see
    /// [`GpuViewTargets::paint`].
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

/// Per-submit eviction: keep an entry unless the submitting owner has
/// stopped recording it. `live` says the submitter still has the view — not
/// that it painted one this frame, which is what makes an undamaged view
/// free to sit a frame out.
///
/// Another owner's entries always survive, since an idle window is not
/// evidence that its views are gone — which is exactly why a *closed* owner
/// has to be retired explicitly through [`GpuViewTargets::retire_owner`],
/// never having a submit to be absent from.
fn keep_target(entry_owner: RenderOwnerId, owner: RenderOwnerId, live: bool) -> bool {
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
    /// `GpuViewTargets::retire_owner` the only thing that frees them.
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
