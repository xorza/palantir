//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]).
//!
//! Two flow shapes:
//!
//! - **Offscreen** — [`Host::run_frame`] (CPU) then
//!   [`Host::render_to_texture`] (GPU submit against a caller-supplied
//!   `wgpu::Texture`). Used by the visual harness and GPU benches that
//!   paint into an offscreen texture, not a swapchain.
//! - **Swapchain** — [`Host::frame_and_render`] is the one-shot:
//!   `run_frame` → acquire `Surface` → submit → `present()`, folding
//!   Suboptimal / Outdated / Lost / Timeout / Validation / Occluded
//!   into a single [`RenderOutcome`]. Hosts that need to inspect the
//!   `FrameReport` between CPU and GPU work can call `run_frame` and
//!   [`Host::render_present`] separately.

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

/// What happened during a swapchain-driving render call. Returned by
/// [`Host::render_present`] and [`FramePresented::outcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOutcome {
    /// Frame painted and presented.
    Painted,
    /// `FrameReport::skip_render()` was true — surface was never
    /// acquired.
    Skipped,
    /// Surface was Suboptimal / Outdated / Lost — already
    /// reconfigured against `config`; caller should request a repaint.
    NeedsReconfigure,
    /// Surface acquire returned Timeout / Validation — transient;
    /// caller should request a repaint.
    NeedsRetry,
    /// Window is occluded — no work to do, no repaint needed until
    /// the host receives an un-occlude event.
    Occluded,
}

impl RenderOutcome {
    /// True for outcomes that mean "you should request another redraw."
    /// `Painted` / `Skipped` / `Occluded` return false — the caller's
    /// repaint loop is driven by `FrameReport::repaint_requested()`
    /// and host events, not by this flag.
    pub fn needs_repaint(self) -> bool {
        matches!(self, Self::NeedsReconfigure | Self::NeedsRetry)
    }
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
    /// thread it back into [`Self::render_to_texture`] or [`Self::render_present`].
    pub fn run_frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.ui.frame(display, self.start.elapsed(), record)
    }

    /// GPU submit against a caller-supplied texture. For visual
    /// harness / offscreen benches that paint into a texture they own
    /// (no swapchain). Swapchain-driven hosts use
    /// [`Self::render_present`].
    ///
    /// On the skip path (`report.damage.is_none()`), copies the
    /// persistent backbuffer onto `target` so callers that always
    /// present still see valid pixels. Clear color is sourced from
    /// `report.clear_color`.
    pub fn render_to_texture(&mut self, target: &wgpu::Texture, report: &FrameReport) {
        profiling::scope!("Host::render_to_texture");
        let Some(damage) = report.damage else {
            self.backend.copy_backbuffer_to_surface(target);
            self.ui.frame_state.mark_submitted();
            return;
        };
        let buffer = self.frontend.build(&self.ui, damage);
        self.backend.submit(
            target,
            report.clear_color,
            buffer,
            damage,
            self.debug_overlay,
        );
        self.ui.frame_state.mark_submitted();
    }

    /// Acquire `surface`'s next frame, paint, present. Folds the
    /// swapchain dance (Suboptimal / Outdated / Lost / Timeout /
    /// Validation / Occluded) into a single [`RenderOutcome`]. On
    /// reconfigure-required variants, calls `surface.configure(_,
    /// config)` before returning so the next acquire has a chance.
    ///
    /// Honors the skip-frame bypass: when `report.skip_render()` is
    /// true, returns `Skipped` without acquiring a surface texture.
    pub fn render_present(
        &mut self,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        report: &FrameReport,
    ) -> RenderOutcome {
        profiling::scope!("Host::render_present");
        if report.skip_render() {
            profiling::finish_frame!();
            return RenderOutcome::Skipped;
        }
        use wgpu::CurrentSurfaceTexture::*;
        let frame = match surface.get_current_texture() {
            Success(f) => f,
            Suboptimal(_) | Outdated | Lost => {
                tracing::warn!("surface acquire: suboptimal / outdated / lost");
                surface.configure(&self.backend.device, config);
                return RenderOutcome::NeedsReconfigure;
            }
            Timeout | Validation => {
                tracing::warn!("surface acquire: timeout / validation");
                return RenderOutcome::NeedsRetry;
            }
            Occluded => return RenderOutcome::Occluded,
        };
        self.render_to_texture(&frame.texture, report);
        frame.present();

        profiling::finish_frame!();
        RenderOutcome::Painted
    }

    /// One-shot: `run_frame` + `render_present`. Derives `Display`'s
    /// physical size from `config.width`/`config.height`; `pixel_snap`
    /// defaults to `true`. Returns whether the host should request
    /// another redraw — folds `FrameReport::repaint_requested()` (e.g.
    /// animation in flight) and `RenderOutcome::needs_repaint()` (e.g.
    /// surface lost) into one bool. Callers that need the underlying
    /// `FrameReport` / `RenderOutcome` stay on the split API.
    pub fn frame_and_render(
        &mut self,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        scale_factor: f32,
        record: impl FnMut(&mut Ui),
    ) -> bool {
        let display =
            Display::from_physical(glam::UVec2::new(config.width, config.height), scale_factor);
        let report = self.run_frame(display, record);
        let outcome = self.render_present(surface, config, &report);
        report.repaint_requested() || outcome.needs_repaint()
    }
}
