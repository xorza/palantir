//! `WindowRenderer` — everything one window owns *above* the shared
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend): its [`Ui`]
//! recorder, a per-window [`Frontend`] (CPU encode/compose scratch), the
//! persistent [`Backbuffer`] (this surface's last-frame pixels), and the
//! per-window frame-scheduling clock + occlusion state.
//!
//! What every window shares splits two ways: the GPU resources — render
//! pipelines, glyph + gradient atlases, the image texture cache, the
//! device/queue — live on the **one** shared `WgpuBackend` the host
//! passes into every method; the GPU-agnostic resources — frame arena,
//! render caches, shaper, GPU-stats handle — live on the [`RenderContext`]
//! this window's `Ui`/`Frontend` were cloned from. So N windows render
//! through one GPU renderer; each `WindowRenderer` carries only this
//! window's data.
//!
//! Single public entry: [`WindowRenderer::frame`] — runs the CPU passes,
//! acquires the swapchain texture, submits through the shared backend,
//! presents, and folds the acquire dance into a [`FramePresent`] schedule.

use std::time::Instant;

use crate::renderer::backend::{Backbuffer, WgpuBackend};
use crate::renderer::context::RenderContext;
use crate::renderer::frontend::Frontend;
use crate::ui::Ui;
use crate::window::WindowToken;
use crate::{Display, FrameReport, FrameStamp};

/// Per-window state driving the shared [`WgpuBackend`]. Built by
/// [`Self::new`] from the shared [`RenderContext`]; owns no GPU resources
/// except its own [`Backbuffer`].
pub struct WindowRenderer {
    pub ui: Ui,
    /// Per-window CPU encode/compose scratch. Shares the backend's frame
    /// arena (cloned at construction) but keeps its own retained
    /// `RenderBuffer` — this window's draw list.
    pub(crate) frontend: Frontend,
    /// This window's persistent off-screen render target — its last
    /// frame's pixels, kept for `LoadOp::Load` damage. The only
    /// per-surface GPU resource; lent to [`WgpuBackend::submit`] each
    /// frame. Lazily created on first submit, recreated on resize / format
    /// change.
    pub(crate) backbuffer: Option<Backbuffer>,
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
    /// Last physical size we actually called `surface.configure` for.
    /// Resize handlers mutate `SurfaceConfiguration` directly; the
    /// next `frame()` notices the mismatch and reconfigures once.
    /// Coalesces compositor configure bursts (Wayland repeats the
    /// configure on focus/output changes, and identical events
    /// otherwise back-to-back) into a single GPU reallocation — see
    /// wgpu #7447 for the 100ms+ stalls `surface.configure` triggers.
    /// `None` until the first paint forces a baseline.
    configured: Option<glam::UVec2>,
    /// Color format of the last target this window rendered to. A format
    /// flip (window moved to an HDR output) changes nothing the `Ui`
    /// tracks — same size, same scene — so without noticing it here an
    /// unchanged scene would damage-`Skip` and copy the stale-format
    /// backbuffer. `frame` / `frame_offscreen` compare against it and
    /// force a full repaint on change (see [`Self::note_format`]).
    /// `None` until the first paint.
    last_format: Option<wgpu::TextureFormat>,
}

impl WindowRenderer {
    /// Build a per-window renderer from the shared [`RenderContext`] (its
    /// `Ui` + `Frontend` clone the context's shaper / frame arena / caches
    /// / GPU-stats handle). Independent of the GPU backend — that's only
    /// needed later, per frame. Owns nothing on the GPU but its backbuffer,
    /// created lazily on the first submit.
    pub(crate) fn new(ctx: &RenderContext) -> Self {
        Self {
            ui: ctx.make_ui(),
            frontend: ctx.make_frontend(),
            backbuffer: None,
            start: Instant::now(),
            occluded: false,
            occluded_at: None,
            configured: None,
            last_format: None,
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

    /// Detect a color-format change against the last frame's target and,
    /// on change, force this frame to repaint fully. A format flip changes
    /// nothing the `Ui` tracks (same size, same scene), so an unchanged
    /// scene would otherwise damage-`Skip` and copy the stale-format
    /// backbuffer. `mark_pending` forces a full record + clear (so submit,
    /// not the copy path, runs); the shared backend builds the new
    /// format's pipelines lazily and the backbuffer self-heals. Resetting
    /// `configured` forces a windowed surface reconfigure at the new
    /// format. Runs every frame — a no-op once the format is steady.
    fn note_format(&mut self, format: wgpu::TextureFormat) {
        if self.last_format != Some(format) {
            self.last_format = Some(format);
            self.ui.frame_state.mark_pending();
            self.configured = None;
        }
    }

    /// Swapchain one-shot: run CPU + GPU + present through the shared
    /// `gpu`. Folds the acquire dance (Suboptimal / Outdated / Lost /
    /// Timeout / Validation / Occluded) into the returned schedule.
    /// Reconfigure-required variants call `surface.configure` before
    /// returning. Skip frames bypass surface acquisition entirely.
    ///
    /// All per-frame swapchain inputs ride in on [`FrameTarget`]: the
    /// surface + its config (which alone defines the physical size), the
    /// display knobs, and the live sibling-window set. `Display` is built
    /// from the config here, so its size can never disagree with the
    /// surface's.
    pub fn frame(
        &mut self,
        gpu: &mut WgpuBackend,
        target: FrameTarget<'_>,
        record: impl FnMut(&mut Ui),
        pre_present: impl FnOnce(),
    ) -> FramePresent {
        // Bracket the body with a Tracy *discontinuous* frame so the
        // frame strip shows actual work duration, not the gap between
        // back-to-back `finish_frame!()` ticks (which counts idle time
        // between user input as one giant "lagging" frame).
        #[cfg(feature = "profile-with-tracy")]
        let _tracy_frame = tracy_client::non_continuous_frame!("frame");
        profiling::scope!("WindowRenderer::frame");

        if self.occluded {
            return FramePresent::Idle;
        }

        let FrameTarget {
            surface,
            config,
            scale_factor,
            refresh_millihertz,
            live_windows,
        } = target;

        // The surface config is the single source of truth for the
        // physical size; `Display` is derived from it so the two can't
        // drift apart.
        let display = Display {
            physical: glam::UVec2::new(config.width, config.height),
            scale_factor,
            pixel_snap: true,
            refresh_millihertz,
        };

        // Refresh the snapshot `Ui::window_open` answers from. Retained
        // Vec, capacity reused — alloc-free once the window set is steady.
        self.ui.live_windows.clear();
        self.ui.live_windows.extend_from_slice(live_windows);

        // Force a full repaint + surface reconfigure if the swapchain
        // format changed (must run before the reconfigure block + cpu_frame).
        self.note_format(config.format);

        // Reconfigure-on-demand: callers update `config.width/height`
        // freely (resize events) without paying for a `surface.configure`
        // per event. We notice the mismatch here, reallocate once, and
        // record the new size. First-paint takes the same path because
        // `configured` starts `None`.
        if self.configured != Some(display.physical) {
            gpu.configure_surface(surface, config);
            self.configured = Some(display.physical);
        }

        let report = self.cpu_frame(display, record);
        self.present(gpu, surface, config, report, pre_present)
    }

    /// CPU half — `Ui::frame` → record → measure / arrange / cascade /
    /// damage. Returns the host-facing [`FrameReport`]; thread it back
    /// into [`Self::render_to_texture`]. Internal split for benches and
    /// the visual harness; production callers use [`Self::frame`].
    pub(crate) fn cpu_frame(
        &mut self,
        display: Display,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        // Ui::frame clears the shared Rc arena at the top of the record
        // cycle — the same Rc the frontend + shared backend hold.
        self.ui
            .frame(FrameStamp::new(display, self.start.elapsed()), record)
    }

    /// GPU submit against a caller-supplied texture, through the shared
    /// `gpu`. On `RenderPlan::Skip`, copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid pixels.
    /// Internal split for benches and the visual harness; production
    /// callers use [`Self::frame`].
    pub(crate) fn render_to_texture(
        &mut self,
        gpu: &mut WgpuBackend,
        target: &wgpu::Texture,
        report: &FrameReport,
    ) {
        profiling::scope!("WindowRenderer::render_to_texture");
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
            gpu.copy_backbuffer_to_surface(&mut self.backbuffer, target);
            self.ui.frame_state.mark_submitted();
            return;
        };
        let buffer = self.frontend.build(&self.ui, plan);
        gpu.submit(
            &mut self.backbuffer,
            target,
            buffer,
            plan,
            self.ui.debug_overlay,
        );
        self.ui.frame_state.mark_submitted();
    }

    fn present(
        &mut self,
        gpu: &mut WgpuBackend,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        report: FrameReport,
        pre_present: impl FnOnce(),
    ) -> FramePresent {
        let repaint = if report.skip_render() {
            report.repaint_requested()
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            match surface.get_current_texture() {
                Success(frame) => {
                    self.render_to_texture(gpu, &frame.texture, &report);
                    // Compositor hook (winit's `Window::pre_present_notify`)
                    // — required on Wayland to schedule the next frame
                    // callback. Without it, `RedrawRequested` throttling
                    // breaks and interactive resize / animation lag
                    // behind the compositor's configure cadence. See
                    // winit #2609, slint #4200.
                    pre_present();
                    frame.present();
                    report.repaint_requested()
                }
                Suboptimal(_) | Outdated | Lost => {
                    tracing::warn!("surface acquire: suboptimal / outdated / lost");
                    gpu.configure_surface(surface, config);
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

/// Every per-frame swapchain input [`WindowRenderer::frame`] needs from
/// the windowing host, bundled into one borrowed argument. The surface
/// `config` is the single source of truth for the physical pixel size —
/// `WindowRenderer::frame` derives `Display.physical` from it, so the
/// size is never passed (or asserted) twice.
#[derive(Debug)]
pub struct FrameTarget<'a> {
    /// Swapchain surface to acquire + present this frame.
    pub surface: &'a wgpu::Surface<'static>,
    /// Its configuration; `width`/`height` define the physical size.
    pub config: &'a wgpu::SurfaceConfiguration,
    /// Logical→physical DPR scale for this window's current monitor.
    pub scale_factor: f32,
    /// Monitor refresh in millihertz (sets the repaint-wake coalesce
    /// floor so timed wakes never out-pace the panel), or `None` when the
    /// host can't determine it.
    pub refresh_millihertz: Option<u32>,
    /// Tokens of the windows live as of this frame's start — copied into
    /// the `Ui` so [`Ui::window_open`](crate::ui::Ui::window_open) answers
    /// without the `Ui` mirroring host state.
    pub live_windows: &'a [WindowToken],
}

/// WindowRenderer scheduling hint returned by [`WindowRenderer::frame`]. Three
/// mutually-exclusive states the event loop must service:
///
/// - [`Self::Immediate`] — call `request_redraw` right away
///   (animation in flight, surface lost, occlusion change).
/// - [`Self::At`] — schedule a wake at this `Instant` via
///   `ControlFlow::WaitUntil`. Used for time-driven UI like tooltip
///   delays where idle pixels don't change but a frame is still
///   needed at a known moment.
/// - [`Self::Idle`] — nothing pending; sleep until the next input.
#[derive(Clone, Copy, Debug)]
pub enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    //! The per-window offscreen render entry. The bundling host that owns
    //! a backend + this window lives in
    //! [`OffscreenHost`](crate::offscreen_host::OffscreenHost).

    use crate::window_renderer::*;

    impl WindowRenderer {
        /// Offscreen one-shot: run CPU + GPU against a caller-supplied
        /// texture (no swapchain acquire), through the shared `gpu`.
        /// `Display`'s physical size is derived from `target.size()`.
        /// Driven by [`OffscreenHost`](crate::offscreen_host::OffscreenHost).
        pub(crate) fn frame_offscreen(
            &mut self,
            gpu: &mut WgpuBackend,
            target: &wgpu::Texture,
            scale_factor: f32,
            record: impl FnMut(&mut Ui),
        ) {
            let size = target.size();
            let display =
                Display::from_physical(glam::UVec2::new(size.width, size.height), scale_factor);
            // Force a full repaint when the target's format changes
            // (offscreen has no surface to reconfigure).
            self.note_format(target.format());
            let report = self.cpu_frame(display, record);
            self.render_to_texture(gpu, target, &report);
        }
    }
}
