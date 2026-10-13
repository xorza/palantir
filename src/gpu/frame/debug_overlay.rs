//! Debug-overlay GPU buffers: the full-viewport dim quad (before partial
//! passes, when `DebugOverlayConfig::dim_undamaged`) and the damage-rect
//! outline quads (after the backbuffer-to-surface copy, when `damage_rect`).
//! Both ride the quad pipeline's no-stencil base pipeline and bind group,
//! passed in as an [`OverlayDraw`].

use crate::damage::Damage;
use crate::damage::region::DAMAGE_RECT_CAP;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::quad_pipeline::QuadBinding;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::surface::viewport::ViewportPush;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use glam::Vec2;
use tinyvec::ArrayVec;

/// Damage-rect outline stroke color: bright opaque red, not theme-driven.
const DAMAGE_OVERLAY_COLOR: RgbaF32 = RgbaF32::srgb(1.0, 0.0, 0.0);

/// Damage-rect outline stroke width in logical px, scaled by
/// `Display::scale_factor()` at submit.
const DAMAGE_OVERLAY_STROKE_WIDTH: f32 = 2.0;

/// Gap between outline and damage edge in logical px. `Partial` rects outset by
/// it so thin damage (a 1px caret) stays visible; the full-viewport outline
/// insets by it to stay on-screen.
const DAMAGE_OVERLAY_GAP: f32 = 1.0;

/// Linear-space alpha of the `dim_undamaged` fill. With premultiplied blending
/// 40% alpha leaves 60% of the underlying pixel per Partial frame.
const DIM_ALPHA: f32 = 0.4;

/// What every debug-overlay draw binds: the no-stencil solid quad pipeline with
/// the gradient group, and the viewport.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayDraw<'a> {
    pub(crate) binding: QuadBinding<'a>,
    pub(crate) viewport: ViewportPush,
}

impl<'a> OverlayDraw<'a> {
    /// Bind, push the viewport (wgpu rejects `set_immediates` before a pipeline is
    /// bound), then draw `count` instances of `buffer`.
    fn quads(self, pass: &mut wgpu::RenderPass<'a>, buffer: &'a wgpu::Buffer, count: u32) {
        self.binding.bind(pass, buffer);
        self.viewport.push_into(pass);
        pass.draw(0..4, 0..count);
    }
}

#[derive(Debug)]
pub(crate) struct DebugOverlay {
    /// Single-instance translucent-black full-viewport quad, drawn with
    /// `LoadOp::Load` before partial passes so undamaged regions fade across frames
    /// while repainted damage stays bright.
    dim: SingleQuadBuffer,
    /// Damage-rect outline quads (transparent fill, red stroke), drawn on the
    /// swapchain texture after the backbuffer-to-surface copy, so no ghosts.
    /// [`DynamicBuffer`] grows it to the region's rect count.
    damage_rects: DynamicBuffer<Quad>,
    /// Outline quads this frame uploaded; `0` skips the overlay pass.
    damage_rect_count: u32,
}

impl DebugOverlay {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            dim: SingleQuadBuffer::new(device, "palantir.quad.dim"),
            // Starts at 8 quads to avoid tiny early regrows.
            damage_rects: DynamicBuffer::<Quad>::vertex(device, "palantir.quad.overlay", 8),
            damage_rect_count: 0,
        }
    }

    /// Upload one full-viewport translucent-black quad ([`DIM_ALPHA`]) unless the
    /// buffer already holds it.
    pub(crate) fn upload_dim(&mut self, ctx: &mut GpuCtx<'_>, viewport: Vec2) {
        let q = Quad {
            rect: Rect::new(0.0, 0.0, viewport.x, viewport.y),
            fill: RgbaF32::new(0.0, 0.0, 0.0, DIM_ALPHA).into(),
            ..Default::default()
        };
        self.dim.upload(ctx, q);
    }

    /// Draw the single dim quad; the dim pass has no stencil, so the no-stencil
    /// pipeline is always correct.
    pub(crate) fn draw_dim<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, draw: OverlayDraw<'a>) {
        draw.quads(pass, self.dim.buffer(), 1);
    }

    /// Build and upload this frame's damage-rect outline quads: `Partial` gives one
    /// per region rect, `Full` one full-viewport outline. Staging is stack-bounded
    /// (`DAMAGE_RECT_CAP`), so steady-state frames are alloc-free.
    pub(crate) fn upload_damage_rects(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        plan: RenderPlan,
        buffer: &RenderBuffer,
    ) {
        let scale = buffer.display.scale_factor();
        let gap_px = (DAMAGE_OVERLAY_GAP * scale).max(1.0);
        let stroke_color = RgbaF16::from(DAMAGE_OVERLAY_COLOR);
        let stroke_width = DAMAGE_OVERLAY_STROKE_WIDTH * scale;
        let outline = |rect: Rect| Quad {
            rect,
            fill: RgbaF16::TRANSPARENT,
            stroke_color,
            stroke_width,
            ..Default::default()
        };
        let mut quads: ArrayVec<[Quad; DAMAGE_RECT_CAP]> = ArrayVec::default();
        match plan.damage {
            Damage::Partial(damage) => {
                // Outset, not inset: damage thinner than `2 * gap_px` (a 1px caret) would
                // collapse to zero area. The pass is unscissored and the surface clips the
                // spill.
                for r in damage.region.iter_rects() {
                    quads.push(outline(r.scaled_by(scale, true).inflated(gap_px)));
                }
            }
            // The full-viewport outline insets instead: outsetting would push it off-screen.
            Damage::Full => {
                let physical = buffer.display.physical.as_vec2();
                quads.push(outline(
                    Rect::new(0.0, 0.0, physical.x, physical.y).deflated(gap_px),
                ));
            }
        }
        self.damage_rects.upload_instances(ctx, quads.as_slice());
        self.damage_rect_count = quads.len() as u32;
    }

    /// Forget the last upload, for a frame that draws no damage rects.
    pub(crate) const fn clear_damage_rects(&mut self) {
        self.damage_rect_count = 0;
    }

    /// Whether this frame uploaded outlines for [`Self::draw_damage_rects`].
    pub(crate) const fn has_damage_rects(&self) -> bool {
        self.damage_rect_count > 0
    }

    /// Draw this frame's damage-rect outlines in the post-copy overlay pass on the
    /// swapchain texture (no stencil, no scissor).
    pub(crate) fn draw_damage_rects<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        draw: OverlayDraw<'a>,
    ) {
        draw.quads(pass, &self.damage_rects.buffer, self.damage_rect_count);
    }
}
