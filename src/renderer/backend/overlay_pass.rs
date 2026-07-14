//! Debug-overlay GPU buffers: the full-viewport dim quad (drawn
//! before partial passes when `DebugOverlayConfig::dim_undamaged`
//! is on) and the damage-rect outline quads (drawn after the
//! backbuffer→surface copy when `damage_rect` is on). Both ride
//! the quad pipeline's no-stencil base pipeline + bind group —
//! `WgpuBackend::run_dim_pass` / `draw_overlays` pass those
//! references through.
//!
//! Lives in its own module so the GPU resources, upload helpers,
//! and the three appearance constants are kept together. Apps that
//! never enable debug overlays still allocate these buffers (a few
//! hundred bytes total) but never upload to them.

use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::{
    primitives::{
        color::{Color, ColorF16},
        corners::Corners,
        rect::Rect,
        size::Size,
        spacing::Spacing,
    },
};
use glam::Vec2;
use tinyvec::ArrayVec;

/// Stroke color for the damage-rect overlay outline. Bright opaque
/// red — picked for contrast against any UI palette, not
/// theme-driven.
const DAMAGE_OVERLAY_COLOR: Color = Color::rgb(1.0, 0.0, 0.0);

/// Stroke width for the damage-rect overlay outline, in logical
/// pixels. Multiplied by `scale_factor` at submit time.
const DAMAGE_OVERLAY_STROKE_WIDTH: f32 = 2.0;

/// Gap between the overlay outline and the damage edge, in logical
/// pixels. `Partial` rects outset by this (so thin damage like a 1px
/// text caret still gets a visible box instead of collapsing to zero
/// width); the full-viewport outline insets by it to stay on-screen.
const DAMAGE_OVERLAY_GAP: f32 = 1.0;

/// Linear-space alpha of the `dim_undamaged` fill. Premultiplied-alpha
/// blending means the rgb channel doubles as the "remaining brightness"
/// multiplier: 40% alpha → 60% of the underlying pixel survives each
/// Partial frame.
const DIM_ALPHA: f32 = 0.4;

#[derive(Debug)]
pub(crate) struct DebugOverlay {
    /// Single-instance buffer holding a translucent-black full-viewport
    /// quad. Drawn into the backbuffer with `LoadOp::Load` before any
    /// partial-damage passes when `DebugOverlayConfig::dim_undamaged` is
    /// on, so each Partial frame darkens prior pixels and the undamaged
    /// region fades to black across frames while the damage region —
    /// repainted at full brightness — stays bright.
    dim_buffer: wgpu::Buffer,
    /// Multi-instance buffer holding damage-rect outline quads
    /// (transparent fill, red stroke per damaged rect). Drawn onto
    /// the swapchain texture *after* the backbuffer→surface copy, so
    /// it never touches the backbuffer and produces no ghosts. Only
    /// written when `DebugOverlayConfig::damage_rect` is on;
    /// [`DynamicBuffer`] grows it to fit the region's rect count.
    overlay_buffer: DynamicBuffer<Quad>,
}

impl DebugOverlay {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let dim_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aperture.quad.dim"),
            size: std::mem::size_of::<Quad>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // `upload_overlays` grows it on demand when the damage region
        // carries more rects (8-quad start avoids tiny early regrows).
        let overlay_buffer = DynamicBuffer::<Quad>::vertex(device, "aperture.quad.overlay", 8);
        Self {
            dim_buffer,
            overlay_buffer,
        }
    }

    /// Upload one full-viewport translucent-black quad ([`DIM_ALPHA`])
    /// to `dim_buffer`.
    pub(crate) fn upload_dim(&self, ctx: &mut GpuCtx<'_>, viewport: Vec2) {
        let q = Quad {
            rect: Rect {
                min: Vec2::ZERO,
                size: Size {
                    w: viewport.x,
                    h: viewport.y,
                },
            },
            fill: Color::linear_rgba(0.0, 0.0, 0.0, DIM_ALPHA).into(),
            corners: Corners::default(),
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            ..Default::default()
        };
        ctx.write(&self.dim_buffer, 0, bytemuck::bytes_of(&q));
    }

    /// Draw the single dim quad. The dim pass runs without a stencil
    /// attachment (uniform dim across the viewport), so the
    /// no-stencil pipeline is always correct here.
    pub(crate) fn draw_dim<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        quad_base: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
        viewport: &ViewportPush,
    ) {
        draw_quads(pass, quad_base, gradient_bg, viewport, &self.dim_buffer, 1);
    }

    /// Build + upload this frame's damage-rect outline quads: `Partial`
    /// contributes one per region rect, `Full` a single full-viewport
    /// outline. Returns the instance count for [`Self::draw_overlays`];
    /// `0` means nothing survived and the caller skips the overlay
    /// pass. All quads ride one instanced draw inside one pass, so a
    /// single belt write covers them.
    pub(crate) fn upload_damage_rects(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        plan: RenderPlan,
        buffer: &RenderBuffer,
    ) -> u32 {
        let gap_px = (DAMAGE_OVERLAY_GAP * buffer.scale).max(1.0);
        let stroke_width = DAMAGE_OVERLAY_STROKE_WIDTH * buffer.scale;
        let mut rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]> = Default::default();
        match plan.kind {
            RenderKind::Partial { region } => {
                // Outset, not inset: damage rects can be thinner than
                // `2 * gap_px` (a 1px text caret), and insetting would
                // collapse them to zero area — no outline drawn. An
                // outset box always survives and brackets the damage
                // from just outside. The overlay pass is unscissored
                // and the surface clips, so spilling a few px past the
                // damage edge is fine.
                for r in region.iter_rects() {
                    rects.push(r.scaled_by(buffer.scale, true).inflated(gap_px));
                }
            }
            // The full-viewport outline insets instead: outsetting it
            // would push the whole box off-screen, leaving only a
            // half-clipped edge line.
            RenderKind::Full => rects.push(
                Rect {
                    min: Vec2::ZERO,
                    size: Size::new(buffer.viewport_phys_f.x, buffer.viewport_phys_f.y),
                }
                .deflated_by(Spacing::all(gap_px)),
            ),
        }
        self.upload_overlays(ctx, &rects, DAMAGE_OVERLAY_COLOR, stroke_width);
        rects.len() as u32
    }

    /// Upload one or more damage-rect outline quads (stroked rects in
    /// physical px, transparent fill). [`DynamicBuffer`] grows the
    /// buffer when needed; the staging uses stack-bounded scratch
    /// (≤ `DAMAGE_RECT_CAP`) so steady-state frames are alloc-free.
    fn upload_overlays(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        rects: &[Rect],
        stroke_color: Color,
        stroke_width: f32,
    ) {
        let stroke_color_f16: ColorF16 = stroke_color.into();
        let mut quads: ArrayVec<[Quad; DAMAGE_RECT_CAP]> = Default::default();
        for r in rects {
            quads.push(Quad {
                rect: *r,
                fill: ColorF16::TRANSPARENT,
                corners: Corners::default(),
                stroke_color: stroke_color_f16,
                stroke_width,
                ..Default::default()
            });
        }
        self.overlay_buffer.upload_instances(ctx, quads.as_slice());
    }

    /// Draw `count` damage-rect outline quads. Used in the post-copy
    /// overlay pass on the swapchain texture (no stencil attachment,
    /// no scissor).
    pub(crate) fn draw_overlays<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        quad_base: &'a wgpu::RenderPipeline,
        gradient_bg: &'a wgpu::BindGroup,
        viewport: &ViewportPush,
        count: u32,
    ) {
        draw_quads(
            pass,
            quad_base,
            gradient_bg,
            viewport,
            &self.overlay_buffer.buffer,
            count,
        );
    }
}

/// Shared draw tail of [`DebugOverlay::draw_dim`] /
/// [`DebugOverlay::draw_overlays`]: bind the supplied quad pipeline's
/// no-stencil base + gradient group 0, push the shared viewport
/// immediate (both overlay passes run standalone, so no inherited
/// immediate state — and wgpu rejects `set_immediates` before a
/// pipeline is bound), then draw `count` instances of `buffer`.
fn draw_quads<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    quad_base: &'a wgpu::RenderPipeline,
    gradient_bg: &'a wgpu::BindGroup,
    viewport: &ViewportPush,
    buffer: &'a wgpu::Buffer,
    count: u32,
) {
    pass.set_pipeline(quad_base);
    pass.set_bind_group(0, gradient_bg, &[]);
    viewport.push_into(pass);
    pass.set_vertex_buffer(0, buffer.slice(..));
    pass.draw(0..4, 0..count);
}
