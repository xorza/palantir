//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]).
//!
//! Single public entry: [`Host::frame`]. Runs CPU passes, acquires the
//! next swapchain texture, submits, presents — folding
//! Suboptimal / Outdated / Lost / Timeout / Validation / Occluded into a
//! single "needs repaint" bool. App-owned state lives in the caller's
//! frame-builder closure (capture it) — palantir doesn't carry it.
//!
//! Internal split — [`Host::cpu_frame`] + [`Host::render_to_texture`] —
//! is `pub(crate)`; benches and the visual test harness reach it via
//! [`test_support`] (gated `cfg(any(test, feature = "internals"))`).

use std::time::Instant;

use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::backend::{WgpuBackend, WgpuBackendConfig};
use crate::renderer::caches::RenderCaches;
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::WindowToken;
use crate::{Display, FrameArena, FrameReport, FrameStamp};

/// Host-level construction knobs. Grouped so [`Host::with_options`]
/// has a fixed signature as new GPU-side settings get exposed.
/// `Default`: GPU instrumentation off.
/// `WinitHostConfig` forwards its corresponding fields here.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostConfig {
    /// Opt into GPU instrumentation (timestamp + pipeline-statistics
    /// queries). Off by default because the per-frame readback
    /// round-trip is non-trivial. See
    /// [`WinitHostConfig::collect_gpu_stats`](crate::WinitHostConfig::collect_gpu_stats)
    /// — `WinitHost` forwards its flag through to this one.
    pub collect_gpu_stats: bool,
}

/// Owns the full palantir pipeline: [`Ui`] (record/layout/cascade/damage)
/// plus the CPU [`Frontend`](crate::renderer::frontend::Frontend) and
/// GPU [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The
/// renderer halves are private; reach the recorder via the public
/// [`Host::ui`] field.
pub struct Host {
    pub ui: Ui,
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
    /// Last physical size we actually called `surface.configure` for.
    /// Resize handlers mutate `SurfaceConfiguration` directly; the
    /// next `frame()` notices the mismatch and reconfigures once.
    /// Coalesces compositor configure bursts (Wayland repeats the
    /// configure on focus/output changes, and identical events
    /// otherwise back-to-back) into a single GPU reallocation — see
    /// wgpu #7447 for the 100ms+ stalls `surface.configure` triggers.
    /// `None` until the first paint forces a baseline.
    configured: Option<glam::UVec2>,
}

impl Host {
    /// Canonical ctor: caller supplies the shaper and a [`HostConfig`]
    /// holding every other knob (GPU instrumentation opt-in). `WinitHost`
    /// delegates here from `WinitHostConfig`.
    pub fn with_options(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
        config: HostConfig,
    ) -> Self {
        let HostConfig { collect_gpu_stats } = config;
        // One canonical frame arena, cloned into every subsystem that
        // touches per-frame mesh / polyline bytes. Each Rc-clone is
        // cheap; runtime borrow-checking via RefCell catches any
        // wiring mistake that would double-borrow.
        let caches = RenderCaches::default();
        let frame_arena = FrameArena::default();
        // Single canonical `GpuPassStats` handle — `Ui` owns it (the
        // debug overlay reads through it), and the backend gets a
        // clone only when `collect_gpu_stats` is on. When off, no
        // backend ever writes; readers always see `None`.
        let pass_stats = GpuPassStats::default();
        let backend_sink = collect_gpu_stats.then(|| pass_stats.clone());
        Self {
            ui: Ui::new(
                shaper.clone(),
                frame_arena.clone(),
                caches.clone(),
                pass_stats,
            ),
            frontend: Frontend::new(frame_arena.clone()),
            backend: WgpuBackend::new(
                device,
                queue,
                format,
                shaper,
                frame_arena,
                caches,
                WgpuBackendConfig {
                    pass_stats: backend_sink,
                },
            ),
            start: Instant::now(),
            occluded: false,
            occluded_at: None,
            configured: None,
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

    /// Rebuild the GPU backend for a new swapchain color `format`.
    /// Call when the host observes the surface's format change
    /// mid-session — e.g. the window moves to an HDR / wide-gamut
    /// output and the compositor renegotiates the swapchain, so
    /// `surface.get_capabilities(..)` now reports a different preferred
    /// format. Rebuilds every format-dependent pipeline (the backend
    /// was built against the old format and would otherwise trip the
    /// hard-assert in `WgpuBackend::ensure_backbuffer`). Cheap no-op
    /// when `format` already matches. Forces the next [`Self::frame`] to
    /// reconfigure the surface and repaint in full.
    ///
    /// Caller still owns the surface: update the
    /// `wgpu::SurfaceConfiguration::format` and reconfigure (or let the
    /// next `frame()` reconfigure) so the swapchain texture handed to
    /// the backend actually carries `format`.
    pub fn set_surface_format(&mut self, format: wgpu::TextureFormat) {
        self.backend.recreate_for_format(format);
        // Drop the cached size so `frame()` reconfigures the surface
        // against the new format on the next call.
        self.configured = None;
        // The backbuffer was dropped in the rebuild and the previously
        // presented pixels live in an old-format texture — neither is
        // valid to `LoadOp::Load` or copy from. Mark the last frame
        // un-submitted so `classify_frame` forces a full record + clear
        // next frame (same path a skipped/lost present takes); otherwise
        // an unchanged scene would damage-skip to a `copy_backbuffer`
        // with no backbuffer to copy.
        self.ui.frame_state.mark_pending();
    }

    /// Swapchain one-shot: run CPU + GPU + present. Folds the acquire
    /// dance (Suboptimal / Outdated / Lost / Timeout / Validation /
    /// Occluded) into the returned `repaint_requested` bool — `true`
    /// if the host should request another redraw. Reconfigure-required
    /// variants call `surface.configure(_, config)` before returning.
    /// Skip frames bypass surface acquisition entirely.
    ///
    /// All per-frame swapchain inputs ride in on [`FrameTarget`]: the
    /// surface + its config (which alone defines the physical size), the
    /// display knobs, and the live sibling-window set. `Display` is built
    /// from the config here, so its size can never disagree with the
    /// surface's.
    pub fn frame(
        &mut self,
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
        profiling::scope!("Host::frame");

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

        // Reconfigure-on-demand: callers update `config.width/height`
        // freely (resize events) without paying for a `surface.configure`
        // per event. We notice the mismatch here, reallocate once, and
        // record the new size. First-paint takes the same path because
        // `configured` starts `None`.
        if self.configured != Some(display.physical) {
            self.backend.configure_surface(surface, config);
            self.configured = Some(display.physical);
        }

        let report = self.cpu_frame(display, record);
        self.present(surface, config, report, pre_present)
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
        // Ui::frame clears its own Rc-shared arena at the top of the
        // record cycle — the same Rc the frontend + backend hold.
        self.ui
            .frame(FrameStamp::new(display, self.start.elapsed()), record)
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
        pre_present: impl FnOnce(),
    ) -> FramePresent {
        let repaint = if report.skip_render() {
            report.repaint_requested()
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            match surface.get_current_texture() {
                Success(frame) => {
                    self.render_to_texture(&frame.texture, &report);
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

/// Every per-frame swapchain input [`Host::frame`] needs from the
/// windowing host, bundled into one borrowed argument. The surface
/// `config` is the single source of truth for the physical pixel size —
/// `Host::frame` derives `Display.physical` from it, so the size is never
/// passed (or asserted) twice.
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
#[derive(Clone, Copy, Debug)]
pub enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    //! Test/bench reach-in surface for `Host` — the single gated
    //! entry point for offscreen frames and GPU instrumentation.
    //! Production code uses the `frame_stats` debug overlay on `Ui`
    //! to surface GPU timings; benches sample the underlying
    //! `GpuPassStats` handle directly without going through the
    //! overlay layout pass.

    use crate::host::*;

    impl Host {
        /// Offscreen one-shot: run CPU + GPU against a caller-supplied
        /// texture (no swapchain acquire). `Display`'s physical size is
        /// derived from `target.size()`. For the visual harness and
        /// offscreen benches.
        pub fn frame_offscreen(
            &mut self,
            target: &wgpu::Texture,
            scale_factor: f32,
            record: impl FnMut(&mut Ui),
        ) {
            let size = target.size();
            let display =
                Display::from_physical(glam::UVec2::new(size.width, size.height), scale_factor);
            let report = self.cpu_frame(display, record);
            self.render_to_texture(target, &report);
        }

        /// Cloneable handle to the most-recent GPU instrumentation
        /// sample — same handle the `Ui` debug overlay reads from.
        /// Returns a live handle even when instrumentation is off:
        /// readers just see `None`.
        pub fn gpu_pass_stats(&self) -> &GpuPassStats {
            &self.ui.gpu_pass_stats
        }

        /// Swapchain color format the GPU pipelines are currently built
        /// for. Lets format-change tests confirm
        /// [`Host::set_surface_format`] reached the backend.
        pub fn surface_format(&self) -> wgpu::TextureFormat {
            self.backend.color_format()
        }

        /// Number of images resident in the GPU texture cache. Used by
        /// the format-change test to assert the cache survives the
        /// surgical pipeline rebuild (no re-upload).
        pub fn gpu_image_cache_len(&self) -> usize {
            self.backend.gpu_image_cache_len()
        }
    }
}
