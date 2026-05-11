//! Rendering pipeline, split into a CPU **frontend** (encode + compose,
//! orchestrated by `Frontend`) and a GPU **backend** (`WgpuBackend`):
//!
//! - [`frontend`] owns the per-frame allocations (cmd vec, render buffer)
//!   and turns `&Tree` into `&RenderBuffer`. Pure CPU; no device handles.
//! - [`backend`] consumes `&RenderBuffer` and submits draws. The only
//!   stage that touches a device/queue.
//!
//! [`RenderBuffer`](render_buffer::RenderBuffer) and [`Quad`](quad::Quad)
//! live at this level — they're the frontend↔backend contract. Pure
//! CPU data; no device handles. Other backends (software rasterizer,
//! headless capture) consume `&RenderBuffer` directly. A TUI/text
//! backend would skip the compose step and walk the encoder's
//! `RenderCmdBuffer` itself, since pixel snap and scissor rects don't
//! apply.
//!
//! The public surface is [`Renderer`], which owns both halves and
//! drives them from one [`Ui::run_frame`](crate::ui::Ui::run_frame)
//! result.
pub(crate) mod backend;
pub(crate) mod frontend;
pub(crate) mod quad;
pub(crate) mod render_buffer;

use crate::primitives::color::Color;
use crate::renderer::backend::WgpuBackend;
use crate::renderer::frontend::{Frontend, RecordedFrame};
use crate::text::TextShaper;

/// Owns both halves of the rendering pipeline — the CPU
/// [`Frontend`](frontend::Frontend) (encode + compose) and the GPU
/// [`WgpuBackend`](backend::WgpuBackend) — so a host gets one type to
/// hold and one [`Self::render`] call per frame.
///
/// The frontend allocations (cmd buffer, render buffer, gradient
/// atlas) are persistent across frames, so steady-state rendering is
/// heap-alloc-free after warmup.
pub struct Renderer {
    pub(crate) frontend: Frontend,
    pub(crate) backend: WgpuBackend,
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            frontend: Frontend::default(),
            backend: WgpuBackend::new(device, queue, format),
        }
    }

    /// Install the shared shaper handle. Pass the same [`TextShaper`]
    /// to `Ui` so layout-time measurement and rasterization see one
    /// buffer cache. Without it, text rendering is silently skipped.
    pub fn set_text_shaper(&mut self, shaper: TextShaper) {
        self.backend.set_text_shaper(shaper);
    }

    /// Run the CPU paint stage (encode + compose) and submit to GPU.
    /// On the `Skip` damage path the GPU pass is bypassed entirely and
    /// the persistent backbuffer is copied straight to `surface_tex`.
    pub fn render(&mut self, surface_tex: &wgpu::Texture, clear: Color, frame: RecordedFrame<'_>) {
        self.frontend.build(
            frame.forest,
            frame.results,
            frame.cascades,
            frame.damage_filter(),
            &frame.display,
        );
        self.backend.submit(
            surface_tex,
            clear,
            &self.frontend.composer.buffer,
            &mut self.frontend.gradient_atlas,
            frame.damage,
            frame.debug_overlay,
            &frame.frame_state,
        );
    }
}
