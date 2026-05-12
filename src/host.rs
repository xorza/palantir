//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]). One type to hold; one [`Host::run_frame`] +
//! [`Host::render`] pair per frame.
//!
//! The two-stage split (`run_frame` → CPU work; `render` → GPU
//! submit) lets the host bail out between the two on `Skip` frames —
//! no `surface.get_current_texture()`, no submit, no present — and
//! also on host-side errors (surface acquire failure, occluded
//! window). The per-frame paint plan flows through the
//! [`FrameReport`] returned from `run_frame`; callers thread it into
//! `render` as `&report`.

use std::time::Instant;

use crate::debug_overlay::DebugOverlayConfig;
use crate::renderer::backend::WgpuBackend;
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::{Display, FrameReport};

/// Owns the full palantir pipeline: [`Ui`] (record/layout/cascade/damage)
/// plus the CPU [`Frontend`](crate::renderer::frontend::Frontend) and
/// GPU [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The
/// renderer halves are private; reach the recorder via the public
/// [`Host::ui`] field.
pub struct Host {
    pub ui: Ui,
    /// Per-frame debug visualizations. Default = all-off. Read by
    /// `render` after `run_frame`; flip flags between frames.
    pub debug_overlay: DebugOverlayConfig,
    pub(crate) frontend: Frontend,
    pub(crate) backend: WgpuBackend,
    /// Monotonic clock anchor — `start.elapsed()` feeds `Ui::frame`
    /// each call so the host doesn't have to thread a clock through.
    pub(crate) start: Instant,
}

impl Host {
    /// Construct with a bundled-fonts shaper shared between the `Ui`
    /// (measurement) and the backend (rasterization) so they hit one
    /// buffer cache.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::with_text(device, queue, format, TextShaper::with_bundled_fonts())
    }

    /// Construct with a caller-supplied shaper. Tests that want to
    /// amortize font loading across many `Host` instances pass a
    /// `clone()` of a shared `TextShaper`. The handle is installed on
    /// both the `Ui` (measurement) and the backend (rasterization).
    pub fn with_text(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
    ) -> Self {
        Self {
            ui: Ui::with_text(shaper.clone()),
            debug_overlay: DebugOverlayConfig::default(),
            frontend: Frontend::default(),
            backend: WgpuBackend::new(device, queue, format, shaper),
            start: Instant::now(),
        }
    }

    /// Drive one CPU frame: `Ui::frame` → record → measure / arrange
    /// / cascade / damage. Returns the host-facing [`FrameReport`];
    /// thread it back into [`Self::render`] as `&report`.
    pub fn run_frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.ui.frame(display, self.start.elapsed(), record)
    }

    /// GPU submit half. Call after [`Self::run_frame`] when the host
    /// wants to paint (i.e. `report.skip_render()` was false). The
    /// clear color is sourced from `report.clear_color` (snapshot of
    /// `Ui.theme.window_clear` at frame time). On both the paint and
    /// skip paths, marks the frame as submitted so the next frame's
    /// damage diff doesn't escalate to `Full` — `Ui::frame` leaves
    /// `frame_state` in `Pending`, and this is the single place that
    /// confirms.
    pub fn render(&mut self, surface_tex: &wgpu::Texture, report: &FrameReport) {
        let Some(damage) = report.damage else {
            // Skip path: nothing changed. Copy the persistent
            // backbuffer onto the swapchain so callers that always
            // present (visual harness, etc.) still see valid pixels.
            // Hosts that pre-check `FrameReport::skip_render` bypass
            // this entirely and never acquire a surface texture.
            self.backend.copy_backbuffer_to_surface(surface_tex);
            self.ui.frame_state.mark_submitted();
            return;
        };
        let buffer = self.frontend.build(&self.ui, damage);
        self.backend.submit(
            surface_tex,
            report.clear_color,
            buffer,
            damage,
            self.debug_overlay,
        );
        self.ui.frame_state.mark_submitted();
    }
}
