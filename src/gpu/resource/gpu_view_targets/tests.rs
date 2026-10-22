use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::resource::gpu_view_targets::{GpuViewTargets, ViewSubmitter};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::render_target_draw::{FrameViews, RenderTargetDraw};
use crate::renderer::render_owner_id::RenderOwners;
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

/// A target painted for a draw's stamp is composited again without a paint; a moved epoch, size, offset or scale paints. Each row is one submit and whether it ran the callback.
#[test]
fn an_unchanged_stamp_skips_the_paint() {
    let gpu = headless_test_gpu();
    let device = &gpu.device;
    let mut targets = GpuViewTargets::new(TextureBinding::new(device));
    let paints = Rc::new(Cell::new(0));
    let paint = GpuPaintRef(Rc::new(RefCell::new(CountingPaint(Rc::clone(&paints)))));
    let id = TextureId::reserve();
    let owner = RenderOwners::default().reserve();
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
                ..draw
            },
            true,
        ),
    ];
    let mut belt = StagingBelt::new(device.clone(), 1 << 12);
    for (at, (draw, painted)) in submits.into_iter().enumerate() {
        let before = paints.get();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let handles = gpu.handles();
        let mut ctx = GpuCtx::new(&handles, &mut belt, &mut encoder);
        targets.paint_gpu_views(
            &mut ctx,
            FrameViews {
                draws: slice::from_ref(&draw),
                live: &[id],
                display_scale: 1.0,
            },
            ViewSubmitter {
                owner,
                now: Duration::from_millis(16 * at as u64),
                text: &shaper,
            },
        );
        assert_eq!(paints.get() - before, u32::from(painted), "submit {at}");
    }
}
