//! Framework-owned off-screen targets for composited `GpuView`s.

use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::textures::ImageTextures;
use crate::renderer::backend::pipeline_utils::texture_bind_group;
use crate::renderer::gpu_view::{GpuFrameCtx, GpuInitCtx};
use crate::renderer::render_buffer::RenderTargetDraw;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

pub(crate) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug, Default)]
pub(crate) struct GpuViewTargets {
    entries: FxHashMap<TextureId, RenderTarget>,
}

impl GpuViewTargets {
    #[allow(clippy::too_many_arguments)]
    #[profiling::function]
    pub(crate) fn paint(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        owner: RenderOwnerId,
        scale: f32,
        now: Duration,
        textures: &mut ImageTextures,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) {
        for draw in frame_targets {
            let target = self.ensure(ctx.device, draw.id, draw.used, owner, textures, layout, sampler);
            let mut paint = draw.paint.0.borrow_mut();
            if !target.initialized {
                profiling::scope!("GpuView::init");
                ctx.encoder.push_debug_group("aperture.gpu_view.init");
                paint.init(&GpuInitCtx {
                    device: ctx.device,
                    target_format: TARGET_FORMAT,
                });
                ctx.encoder.pop_debug_group();
                target.initialized = true;
            }
            let dt = target
                .last_paint
                .map_or(Duration::ZERO, |last| now.saturating_sub(last));
            profiling::scope!("GpuView::paint");
            ctx.encoder.push_debug_group("aperture.gpu_view.paint");
            paint.paint(&mut GpuFrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                encoder: ctx.encoder,
                target: &target.view,
                size_px: draw.used,
                scale,
                dt,
            });
            ctx.encoder.pop_debug_group();
            target.last_paint = Some(now);
        }
        self.entries.retain(|id, target| {
            let keep = keep_target(target.owner, *id, owner, frame_targets);
            if !keep {
                textures.bindings.remove(id);
            }
            keep
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        size: UVec2,
        owner: RenderOwnerId,
        textures: &mut ImageTextures,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> &mut RenderTarget {
        match self.entries.entry(id) {
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                target.owner = owner;
                if target.size != size {
                    let allocated = allocate(device, layout, sampler, size);
                    target.view = allocated.view;
                    textures.bindings.insert(id, allocated.bind_group);
                    target.size = size;
                }
                target
            }
            Entry::Vacant(entry) => {
                let allocated = allocate(device, layout, sampler, size);
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
    initialized: bool,
    last_paint: Option<Duration>,
}

#[derive(Debug)]
struct AllocatedTarget {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

fn allocate(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: UVec2,
) -> AllocatedTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aperture.gpu_view.target"),
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
    let bind_group = texture_bind_group(
        device,
        layout,
        sampler,
        &view,
        "aperture.gpu_view.tex.bg",
    );
    AllocatedTarget { view, bind_group }
}

fn keep_target(
    entry_owner: RenderOwnerId,
    id: TextureId,
    owner: RenderOwnerId,
    frame_targets: &[RenderTargetDraw],
) -> bool {
    entry_owner != owner || frame_targets.iter().any(|draw| draw.id == id)
}

#[cfg(test)]
mod tests {
    use super::keep_target;
    use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
    use crate::renderer::render_buffer::RenderTargetDraw;
    use crate::renderer::render_buffer::owner::RenderOwnerId;
    use crate::renderer::texture_id::TextureId;
    use glam::UVec2;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct NoopPaint;

    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    fn draw(id: u64) -> RenderTargetDraw {
        RenderTargetDraw {
            id: TextureId(id),
            used: UVec2::ONE,
            paint: GpuPaintRef(Rc::new(RefCell::new(NoopPaint))),
        }
    }

    fn evicted(
        entries: &[(u64, RenderOwnerId)],
        owner: RenderOwnerId,
        frame_targets: &[RenderTargetDraw],
    ) -> Vec<u64> {
        entries
            .iter()
            .filter(|(id, entry_owner)| {
                !keep_target(*entry_owner, TextureId(*id), owner, frame_targets)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    #[test]
    fn eviction_is_owner_scoped() {
        let a = RenderOwnerId::reserve();
        let b = RenderOwnerId::reserve();
        let entries = [(1, a), (3, a), (2, b)];
        let cases = [
            (a, vec![draw(1), draw(3)], vec![]),
            (a, vec![draw(1)], vec![3]),
            (b, vec![draw(2)], vec![]),
            (b, vec![], vec![2]),
            (a, vec![], vec![1, 3]),
        ];
        for (owner, frame, expected) in cases {
            assert_eq!(evicted(&entries, owner, &frame), expected);
        }
    }
}
