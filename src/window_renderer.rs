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
//! render caches, shaper, GPU-stats handle — live on the [`HostContext`]
//! this window's `Ui`/`Frontend` were cloned from. So N windows render
//! through one GPU renderer; each `WindowRenderer` carries only this
//! window's data.
//!
//! Two public entries, sharing one CPU + GPU path:
//! [`WindowRenderer::frame`] (to a swapchain surface — acquires, submits,
//! presents, returns a [`FramePresent`] schedule) and
//! [`WindowRenderer::frame_offscreen`] (to a caller-supplied
//! `wgpu::Texture` — no acquire/present, for screenshots / the offscreen
//! host).

use std::time::Instant;

use crate::context::HostContext;
use crate::renderer::backend::{Backbuffer, WgpuBackend};
use crate::renderer::frontend::Frontend;
use crate::ui::Ui;
use crate::{Display, FrameReport, FrameStamp};

/// Per-window state driving the shared [`WgpuBackend`]. Built by
/// [`Self::new`] from the shared [`HostContext`]; owns no GPU resources
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
    /// Build a per-window renderer from the shared [`HostContext`]: its
    /// `Ui` + `Frontend` clone the context's shaper / frame arena / caches /
    /// GPU-stats handle, and the `Ui` shares the context's app-global host
    /// state (live-window set + debug overlay) so all windows agree.
    /// `max_texture_dim` is the device's `max_texture_dimension_2d` (fixed for
    /// its lifetime), handed to the `Frontend` to cap `GpuView` target sizes —
    /// the only GPU fact the CPU pipeline needs. Owns nothing on the GPU but
    /// its backbuffer, created lazily on the first submit.
    pub(crate) fn new(ctx: &HostContext, max_texture_dim: u32) -> Self {
        Self {
            ui: Ui::new(ctx),
            frontend: Frontend::new(ctx.frame_arena.clone(), max_texture_dim),
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
    /// surface + its config (which alone defines the physical size) and the
    /// display knobs. `Display` is built from the config here, so its size
    /// can never disagree with the surface's. (The live-window set + debug
    /// overlay reach the `Ui` through the shared [`HostContext`], not here.)
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
        let present = self.present(gpu, surface, config, report, pre_present);
        profiling::finish_frame!();

        present
    }

    /// Render one frame to a caller-supplied `wgpu::Texture` instead of a
    /// swapchain surface — the texture sibling of [`Self::frame`]. No
    /// acquire/present dance and no [`FramePresent`] schedule; `Display`'s
    /// physical size is derived from `target.size()`. Runs the same CPU +
    /// GPU path (`cpu_frame` → `render_to_texture`) as `frame`. Used by
    /// the offscreen host (visual harness / GPU benches) and available to
    /// any host wanting render-to-texture (screenshots, thumbnails,
    /// offscreen compositing).
    pub fn frame_offscreen(
        &mut self,
        gpu: &mut WgpuBackend,
        target: &wgpu::Texture,
        scale_factor: f32,
        record: impl FnMut(&mut Ui),
    ) {
        let size = target.size();
        let display =
            Display::from_physical(glam::UVec2::new(size.width, size.height), scale_factor);
        // Force a full repaint when the target's format changes (offscreen
        // has no surface to reconfigure).
        self.note_format(target.format());
        let report = self.cpu_frame(display, record);
        self.render_to_texture(gpu, target, &report);
    }

    /// The CPU half of a frame: `Ui::frame` (record → measure / arrange /
    /// cascade / damage) followed, when the frame actually paints, by the
    /// draw-list build (encode → compose → resolve `GpuView`s into the
    /// frontend's buffer). Returns the host-facing [`FrameReport`]; thread it
    /// into the GPU half ([`Self::present`] / [`Self::render_to_texture`]).
    /// No GPU input — the `GpuView` size cap was captured on the `Frontend` at
    /// construction. Shared by [`Self::frame`] (surface) and
    /// [`Self::frame_offscreen`] (texture).
    pub(crate) fn cpu_frame(
        &mut self,
        display: Display,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        // Ui::frame clears the shared Rc arena at the top of the record
        // cycle — the same Rc the frontend + shared backend hold.
        let report = self
            .ui
            .frame(FrameStamp::new(display, self.start.elapsed()), record);
        // Build the draw list now (CPU) when the frame paints — encode,
        // compose, and resolve `GpuView` targets, all reading the now-frozen
        // `Ui` immutably. A Skip frame has no plan and builds nothing.
        if let Some(plan) = report.plan {
            self.frontend.build(&self.ui, plan);
        }
        report
    }

    /// GPU submit against a caller-supplied texture, through the shared
    /// `gpu`. On `RenderPlan::Skip`, copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid pixels.
    /// Shared by [`Self::frame`]'s present path (the acquired swapchain
    /// texture) and [`Self::frame_offscreen`] (an offscreen texture).
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
        // The CPU phase already composed + resolved `GpuView`s into
        // `self.frontend.buffer` (see `cpu_frame`); this is GPU submit only.
        // `buffer` borrows `self.frontend`, `gpu_views` borrows `self.ui` —
        // disjoint fields.
        let debug_overlay = self.ui.debug_overlay();
        gpu.submit(
            &mut self.backbuffer,
            target,
            &self.frontend.buffer,
            plan,
            debug_overlay,
            &mut self.ui.gpu_views,
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
