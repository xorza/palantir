use crate::gpu::gpu_paint::GpuPaint;
use crate::gpu::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::gpu::gpu_paint::gpu_views::{GpuViews, RecordedView, ViewContent};
use crate::gpu::resource::gpu_view_targets::{GpuViewTargets, ViewSubmitter};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::identity::widget_id::WidgetId;
use crate::renderer::render_buffer::render_target_draw::{FrameViews, RenderTargetDraw};
use crate::renderer::render_owner_id::RenderOwners;
use crate::text::shaper::TextShaper;
use glam::UVec2;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::slice;
use std::time::Duration;

#[derive(Debug)]
struct CountingPaint(Rc<Cell<u32>>);

impl GpuPaint for CountingPaint {
    fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {
        self.0.set(self.0.get() + 1);
    }
}

/// A target painted for a draw's stamp is composited again without a paint; a moved epoch, size, offset or scale paints. A clip that scrolls inside one capacity bucket keeps the texture. Each row is one submit, whether it ran the callback, and whether it reallocated.
#[test]
fn an_unchanged_stamp_skips_the_paint_and_a_bucket_keeps_the_texture() {
    let gpu = headless_test_gpu();
    let mut targets = GpuViewTargets::new(TextureBinding::new(&gpu));
    let paints = Rc::new(Cell::new(0));
    let paint = GpuPaintRef(Rc::new(RefCell::new(CountingPaint(Rc::clone(&paints)))));
    let view = WidgetId::from_hash("counted");
    let mut callbacks = GpuViews::default();
    let id = callbacks
        .record(
            RecordedView {
                id: view,
                paint,
                content: ViewContent::Changed,
            },
            1,
        )
        .id;
    let owner = RenderOwners::default().reserve();
    let shaper = TextShaper::new();
    let draw = RenderTargetDraw {
        id,
        used: UVec2::new(32, 24),
        full: UVec2::new(32, 24),
        offset: UVec2::ZERO,
        raster_factor: 1.0,
        owner: view,
        epoch: 1,
    };
    let submits = [
        (draw, true, true),
        (draw, false, false),
        (RenderTargetDraw { epoch: 2, ..draw }, true, false),
        (RenderTargetDraw { epoch: 2, ..draw }, false, false),
        (
            RenderTargetDraw {
                epoch: 2,
                used: UVec2::new(16, 24),
                offset: UVec2::new(16, 0),
                ..draw
            },
            true,
            true,
        ),
        // 12 of 32 buckets to 16, as 16 of 32 did.
        (
            RenderTargetDraw {
                epoch: 2,
                used: UVec2::new(12, 24),
                offset: UVec2::new(20, 0),
                ..draw
            },
            true,
            false,
        ),
        (
            RenderTargetDraw {
                epoch: 2,
                used: UVec2::new(16, 24),
                offset: UVec2::new(16, 0),
                raster_factor: 2.0,
                ..draw
            },
            true,
            false,
        ),
    ];
    let mut texture: Option<wgpu::Texture> = None;
    for (at, (draw, painted, reallocated)) in submits.into_iter().enumerate() {
        let before = paints.get();
        gpu.frame(|ctx| {
            targets.paint_gpu_views(
                ctx,
                FrameViews {
                    draws: slice::from_ref(&draw),
                    live: &[id],
                    display_scale: 1.0,
                },
                &callbacks,
                ViewSubmitter {
                    owner,
                    now: Duration::from_millis(16 * at as u64),
                    text: &shaper,
                },
            );
        });
        assert_eq!(paints.get() - before, u32::from(painted), "submit {at}");
        let now = targets.targets[&id].view.texture().clone();
        assert_eq!(
            texture.as_ref() != Some(&now),
            reallocated,
            "submit {at} reallocation"
        );
        let size = now.size();
        assert_eq!(
            UVec2::new(size.width, size.height),
            draw.capacity(),
            "submit {at}"
        );
        texture = Some(now);
    }
}
