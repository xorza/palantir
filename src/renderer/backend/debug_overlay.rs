//! Debug-overlay GPU buffers: the full-viewport dim quad (drawn
//! before partial passes when `DebugOverlayConfig::dim_undamaged`
//! is on) and the damage-rect outline quads (drawn after the
//! backbuffer→surface copy when `damage_rect` is on). Both ride
//! the quad pipeline's no-stencil base pipeline + bind group —
//! `WgpuBackend::run_dim_pass` / `draw_debug_overlay` pass those
//! references through.
//!
//! Lives in its own module so the GPU resources, upload helpers,
//! and the three appearance constants are kept together. Apps that
//! never enable debug overlays still allocate these buffers (cheap
//! at ~92 B each) but never upload to them.

use crate::primitives::{
    color::{Color, ColorF16},
    corners::Corners,
    rect::Rect,
    size::Size,
};
use crate::renderer::quad::Quad;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use glam::Vec2;
use tinyvec::ArrayVec;

/// Stroke color for the damage-rect overlay outline. Bright opaque
/// red — picked for contrast against any UI palette, not
/// theme-driven.
pub(super) const DAMAGE_OVERLAY_COLOR: Color = Color::rgb(1.0, 0.0, 0.0);

/// Stroke width for the damage-rect overlay outline, in logical
/// pixels. Multiplied by `scale_factor` at submit time.
pub(super) const DAMAGE_OVERLAY_STROKE_WIDTH: f32 = 2.0;

/// How far the overlay rect is inset from the damage rect, in
/// logical pixels. Centers the stroke fully inside the highlighted
/// region.
pub(super) const DAMAGE_OVERLAY_INSET: f32 = 1.0;

pub(super) struct DebugOverlay {
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
    /// written when `DebugOverlayConfig::damage_rect` is on; sized
    /// dynamically by [`Self::upload_overlays`] to fit the region's
    /// rect count.
    overlay_buffer: wgpu::Buffer,
    overlay_capacity: usize,
}

impl DebugOverlay {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let dim_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.quad.dim"),
            size: std::mem::size_of::<Quad>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Sized for one quad up front; `upload_overlays` grows it on
        // demand when the damage region carries more rects.
        let overlay_capacity = 1;
        let overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.quad.overlay"),
            size: (overlay_capacity * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            dim_buffer,
            overlay_buffer,
            overlay_capacity,
        }
    }

    /// Upload one full-viewport translucent-black quad to `dim_buffer`.
    /// `alpha` is the linear-space alpha of the dim fill — `0.4` is
    /// the showcase default. Premultiplied-alpha blending means the
    /// rgb channel doubles as the "remaining brightness" multiplier:
    /// 40% alpha → 60% of the underlying pixel survives.
    pub(super) fn upload_dim(&self, queue: &wgpu::Queue, viewport: Vec2, alpha: f32) {
        let q = Quad {
            rect: Rect {
                min: Vec2::ZERO,
                size: Size {
                    w: viewport.x,
                    h: viewport.y,
                },
            },
            fill: Color::linear_rgba(0.0, 0.0, 0.0, alpha).into(),
            radius: Corners::default(),
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            ..Default::default()
        };
        queue.write_buffer(&self.dim_buffer, 0, bytemuck::bytes_of(&q));
    }

    /// Bind the supplied no-stencil base pipeline + dim buffer and
    /// draw one instance. The dim pass runs without a stencil
    /// attachment (uniform dim across the viewport), so the
    /// no-stencil pipeline is always correct here.
    pub(super) fn draw_dim<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        bind_group: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.dim_buffer.slice(..));
        pass.draw(0..4, 0..1);
    }

    /// Upload one or more damage-rect outline quads (stroked rects in
    /// physical px, transparent fill). Buffer grows to the next power
    /// of two when needed, mirroring the mask buffer's dynamic-resize
    /// pattern; the upload uses stack-bounded scratch
    /// (≤ `DAMAGE_RECT_CAP`) so steady-state frames are alloc-free.
    pub(super) fn upload_overlays(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rects: &[Rect],
        stroke_color: Color,
        stroke_width: f32,
    ) {
        if rects.is_empty() {
            return;
        }
        if rects.len() > self.overlay_capacity {
            self.overlay_capacity = rects.len().next_power_of_two().max(8);
            self.overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.quad.overlay"),
                size: (self.overlay_capacity * std::mem::size_of::<Quad>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let stroke_color_f16: ColorF16 = stroke_color.into();
        let mut quads: ArrayVec<[Quad; DAMAGE_RECT_CAP]> = Default::default();
        for r in rects {
            quads.push(Quad {
                rect: *r,
                fill: ColorF16::TRANSPARENT,
                radius: Corners::default(),
                stroke_color: stroke_color_f16,
                stroke_width,
                ..Default::default()
            });
        }
        queue.write_buffer(
            &self.overlay_buffer,
            0,
            bytemuck::cast_slice(quads.as_slice()),
        );
    }

    /// Bind the supplied no-stencil base pipeline + overlay buffer
    /// and draw `count` instances. Used in the post-copy overlay
    /// pass on the swapchain texture (no stencil attachment, no
    /// scissor).
    pub(super) fn draw_overlays<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        bind_group: &'a wgpu::BindGroup,
        count: u32,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.overlay_buffer.slice(..));
        pass.draw(0..4, 0..count);
    }
}
