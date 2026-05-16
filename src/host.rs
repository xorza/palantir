//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]).
//!
//! Single public entry: [`Host::frame`]. Runs CPU passes, acquires the
//! next swapchain texture, submits, presents — folding
//! Suboptimal / Outdated / Lost / Timeout / Validation / Occluded into a
//! single "needs repaint" bool. Always takes ambient app state (`&mut ()`
//! when there is none) so deep widgets can reach it via
//! [`Ui::app::<T>()`] without explicit threading.
//!
//! Internal split — [`Host::cpu_frame`] + [`Host::render_to_texture`] —
//! is `pub(crate)`; benches and the visual test harness reach it via
//! [`test_support`] (gated `cfg(any(test, feature = "internals"))`).

use std::time::Instant;

use crate::renderer::backend::{DEFAULT_IMAGE_BUDGET_BYTES, WgpuBackend};
use crate::renderer::caches::RenderCaches;
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::{Display, FrameReport, FrameStamp};

/// Owns the full palantir pipeline: [`Ui`] (record/layout/cascade/damage)
/// plus the CPU [`Frontend`](crate::renderer::frontend::Frontend) and
/// GPU [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The
/// renderer halves are private; reach the recorder via the public
/// [`Host::ui`] field.
pub struct Host<T = ()> {
    pub ui: Ui<T>,
    pub(crate) frontend: Frontend,
    pub(crate) backend: WgpuBackend,
    /// Monotonic clock anchor — `start.elapsed()` feeds `Ui::frame`
    /// each call so the host doesn't have to thread a clock through.
    pub(crate) start: Instant,
    /// When true, `frame()` short-circuits to `Idle` without running
    /// `cpu_frame`. Every per-frame Ui flag (damage, repaint_requested,
    /// animation driver state) is naturally preserved because nothing
    /// consumes it. Input still flows through `Ui::on_input` and
    /// accumulates for the first un-occluded frame.
    occluded: bool,
    /// Instant the window went occluded; on resume `start` is shifted
    /// forward by the elapsed hidden duration so anim drivers don't
    /// see a giant `dt` for the gap.
    occluded_at: Option<Instant>,
}

impl<T: 'static> Host<T> {
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
        Self::with_text_and_image_budget(device, queue, format, shaper, DEFAULT_IMAGE_BUDGET_BYTES)
    }

    /// Like [`Self::with_text`] but lets the host pick a GPU image
    /// cache budget up front. Default ([`DEFAULT_IMAGE_BUDGET_BYTES`])
    /// is 256 MB. Can also be adjusted later via
    /// [`Self::set_image_budget_bytes`].
    pub fn with_text_and_image_budget(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
        image_budget_bytes: u64,
    ) -> Self {
        // One canonical frame arena, cloned into every subsystem that
        // touches per-frame mesh / polyline bytes. Each Rc-clone is
        // cheap; runtime borrow-checking via RefCell catches any
        // wiring mistake that would double-borrow.
        let caches = RenderCaches::default();
        let frame_arena = crate::common::frame_arena::FrameArena::default();
        Self {
            ui: Ui::new(shaper.clone(), frame_arena.clone(), caches.clone()),
            frontend: Frontend::new(frame_arena.clone()),
            backend: WgpuBackend::new(
                device,
                queue,
                format,
                shaper,
                frame_arena,
                caches,
                image_budget_bytes,
            ),
            start: Instant::now(),
            occluded: false,
            occluded_at: None,
        }
    }

    /// Drive from the host's window-event handler. While occluded,
    /// `frame()` returns `Idle` without running CPU passes; pending
    /// Ui state (damage, repaint requests, animation deadlines)
    /// survives untouched until the window becomes visible again.
    pub fn set_occluded(&mut self, occluded: bool) {
        match (self.occluded, occluded) {
            (false, true) => self.occluded_at = Some(Instant::now()),
            (true, false) => {
                if let Some(t) = self.occluded_at.take() {
                    self.start += t.elapsed();
                }
            }
            _ => {}
        }
        self.occluded = occluded;
    }

    /// Swapchain one-shot: run CPU + GPU + present. Installs `state` as
    /// ambient app state for the frame; callers without app state pass
    /// `&mut ()`. Folds the acquire dance
    /// (Suboptimal / Outdated / Lost / Timeout / Validation / Occluded)
    /// into the returned `repaint_requested` bool — `true` if the host
    /// should request another redraw. Reconfigure-required variants
    /// call `surface.configure(_, config)` before returning. Skip
    /// frames bypass surface acquisition entirely.
    ///
    /// Derives `Display`'s physical size from `config.width`/`config.height`.
    pub fn frame(
        &mut self,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        scale_factor: f32,
        state: &mut T,
        record: impl FnMut(&mut Ui<T>),
    ) -> FramePresent {
        // Bracket the body with a Tracy *discontinuous* frame so the
        // frame strip shows actual work duration, not the gap between
        // back-to-back `finish_frame!()` ticks (which counts idle time
        // between user input as one giant "lagging" frame).
        #[cfg(feature = "profile-with-tracy")]
        let _tracy_frame = tracy_client::non_continuous_frame!("frame");
        profiling::scope!("Host::frame");

        if self.occluded {
            return FramePresent::Idle;
        }

        let display =
            Display::from_physical(glam::UVec2::new(config.width, config.height), scale_factor);
        let report = self.cpu_frame(display, state, record);
        self.present(surface, config, report)
    }

    /// Offscreen one-shot: run CPU + GPU against a caller-supplied
    /// texture (no swapchain acquire). `Display`'s physical size is
    /// derived from `target.size()`. For the visual harness and
    /// offscreen benches.
    pub fn frame_offscreen(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        state: &mut T,
        record: impl FnMut(&mut Ui<T>),
    ) {
        let size = target.size();
        let display =
            Display::from_physical(glam::UVec2::new(size.width, size.height), scale_factor);
        let report = self.cpu_frame(display, state, record);
        self.render_to_texture(target, &report);
    }

    /// CPU half — `Ui::frame` → record → measure / arrange / cascade /
    /// damage. Returns the host-facing [`FrameReport`]; thread it back
    /// into [`Self::render_to_texture`]. Internal split for benches and
    /// the visual harness; production callers use [`Self::frame`].
    pub(crate) fn cpu_frame(
        &mut self,
        display: Display,
        state: &mut T,
        record: impl FnMut(&mut Ui<T>),
    ) -> FrameReport {
        // Ui::frame clears its own Rc-shared arena at the top of the
        // record cycle — the same Rc the frontend + backend hold.
        self.ui.frame(
            FrameStamp::new(display, self.start.elapsed()),
            state,
            record,
        )
    }

    /// GPU submit against a caller-supplied texture. On
    /// `RenderPlan::Skip`, copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid
    /// pixels. Internal split for benches and the visual harness;
    /// production callers use [`Self::frame`].
    pub(crate) fn render_to_texture(&mut self, target: &wgpu::Texture, report: &FrameReport) {
        profiling::scope!("Host::render_to_texture");
        let size = target.size();
        let display_phys = self.ui.display.physical;
        assert!(
            size.width == display_phys.x && size.height == display_phys.y,
            "render_to_texture: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `cpu_frame` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `cpu_frame`.",
            size.width,
            size.height,
            display_phys.x,
            display_phys.y,
        );
        let Some(plan) = report.plan else {
            self.backend.copy_backbuffer_to_surface(target);
            self.ui.frame_state.mark_submitted();
            return;
        };
        let buffer = self.frontend.build(&self.ui, plan);
        self.backend
            .submit(target, buffer, plan, self.ui.debug_overlay);
        self.ui.frame_state.mark_submitted();
    }

    fn present(
        &mut self,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        report: FrameReport,
    ) -> FramePresent {
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
                    self.backend.configure_surface(surface, config);
                    true
                }
                Timeout | Validation => {
                    tracing::warn!("surface acquire: timeout / validation");
                    true
                }
                // Occlusion is normally handled by the early-out in
                // `frame()` driven by `set_occluded`; if the surface
                // reports it anyway (race with the window event),
                // treat as "nothing to do".
                Occluded => false,
            }
        };

        profiling::finish_frame!();

        if repaint {
            FramePresent::Immediate
        } else if let Some(deadline) = report.repaint_after() {
            FramePresent::At(self.start + deadline)
        } else {
            FramePresent::Idle
        }
    }
}

/// Host scheduling hint returned by [`Host::frame`]. Three
/// mutually-exclusive states the event loop must service:
///
/// - [`Self::Immediate`] — call `request_redraw` right away
///   (animation in flight, surface lost, occlusion change).
/// - [`Self::At`] — schedule a wake at this `Instant` via
///   `ControlFlow::WaitUntil`. Used for time-driven UI like tooltip
///   delays where idle pixels don't change but a frame is still
///   needed at a known moment.
/// - [`Self::Idle`] — nothing pending; sleep until the next input.
pub enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

/// Test/bench surface — exposes the `pub(crate)` CPU/GPU split as
/// public methods so external benches and the visual harness can
/// drive each half independently without going through the
/// swapchain.
#[cfg(any(test, feature = "internals"))]
impl<T: 'static> Host<T> {
    /// CPU half of [`Self::frame`] — runs `Ui::frame` without acquiring a swapchain.
    pub fn cpu_frame_for_test(
        &mut self,
        display: Display,
        state: &mut T,
        record: impl FnMut(&mut Ui<T>),
    ) -> FrameReport {
        self.cpu_frame(display, state, record)
    }

    /// GPU half of [`Self::frame`] against a caller-supplied texture.
    pub fn render_to_texture_for_test(&mut self, target: &wgpu::Texture, report: &FrameReport) {
        self.render_to_texture(target, report);
    }
}
