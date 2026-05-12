//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]).
//!
//! Two flow shapes:
//!
//! - **Offscreen** — [`Host::run_frame`] (CPU) then
//!   [`Host::render_to_texture`] (GPU submit against a caller-supplied
//!   `wgpu::Texture`). Used by the visual harness and offscreen
//!   benches.
//! - **Swapchain** — [`Host::frame_and_render`] is the one-shot:
//!   `run_frame` → acquire `Surface` → submit → `present()`, folding
//!   Suboptimal / Outdated / Lost / Timeout / Validation / Occluded
//!   into a single "needs repaint" bool.

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
    /// Per-frame debug visualizations. Default = all-off. Read during
    /// `render_*`; flip flags between frames.
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

    /// Drive one CPU frame: `Ui::frame` → record → measure / arrange /
    /// cascade / damage. Returns the host-facing [`FrameReport`];
    /// thread it back into [`Self::render_to_texture`].
    pub fn run_frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.ui.frame(display, self.start.elapsed(), record)
    }

    /// GPU submit against a caller-supplied texture. For visual
    /// harness / offscreen benches that paint into a texture they own
    /// (no swapchain). On the skip path (`report.damage.is_none()`),
    /// copies the persistent backbuffer onto `target` so callers that
    /// always present still see valid pixels.
    pub fn render_to_texture(&mut self, target: &wgpu::Texture, report: &FrameReport) {
        profiling::scope!("Host::render_to_texture");
        let size = target.size();
        let display_phys = self.ui.display.physical;
        assert!(
            size.width == display_phys.x && size.height == display_phys.y,
            "render_to_texture: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `run_frame` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `run_frame`.",
            size.width,
            size.height,
            display_phys.x,
            display_phys.y,
        );
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

    /// Swapchain one-shot: run the CPU frame, acquire the next
    /// `surface` texture, submit, present. Folds the acquire dance
    /// (Suboptimal / Outdated / Lost / Timeout / Validation / Occluded)
    /// into the returned `repaint_requested` bool — `true` if the host
    /// should request another redraw (animation in flight, surface
    /// reconfigured, transient acquire failure). Reconfigure-required
    /// variants call `surface.configure(_, config)` before returning.
    /// Skip frames bypass surface acquisition entirely.
    ///
    /// Derives `Display`'s physical size from
    /// `config.width`/`config.height`; `pixel_snap` defaults to `true`.
    /// Callers that need to customize `Display` or inspect the
    /// `FrameReport` between CPU and GPU stay on the split API
    /// (`run_frame` + `render_to_texture` against
    /// `surface.get_current_texture()`).
    pub fn frame_and_render(
        &mut self,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        scale_factor: f32,
        record: impl FnMut(&mut Ui),
    ) -> bool {
        // Bracket the body with a Tracy *discontinuous* frame so the
        // frame strip shows actual work duration, not the gap between
        // back-to-back `finish_frame!()` ticks (which counts idle time
        // between user input as one giant "lagging" frame).
        #[cfg(feature = "profile-with-tracy")]
        let _tracy_frame = tracy_client::non_continuous_frame!("frame");
        profiling::scope!("Host::frame_and_render");

        let display =
            Display::from_physical(glam::UVec2::new(config.width, config.height), scale_factor);
        let report = self.run_frame(display, record);

        let repaint = if report.skip_render() {
            report.repaint_requested()
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            match surface.get_current_texture() {
                Success(frame) => {
                    self.render_to_texture(&frame.texture, &report);
                    frame.present();
                    report.repaint_requested()
                }
                Suboptimal(_) | Outdated | Lost => {
                    tracing::warn!("surface acquire: suboptimal / outdated / lost");
                    surface.configure(&self.backend.device, config);
                    true
                }
                Timeout | Validation => {
                    tracing::warn!("surface acquire: timeout / validation");
                    true
                }
                Occluded => false,
            }
        };

        profiling::finish_frame!();

        repaint
    }
}
