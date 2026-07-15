//! `WindowRenderer` — everything one window owns *above* the shared
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend): its [`Ui`]
//! recorder, a per-window [`Frontend`] (CPU encode/compose scratch), the
//! persistent [`Backbuffer`] (this surface's last-frame pixels), and the
//! per-window frame-scheduling clock + occlusion state.
//!
//! What every window shares splits two ways: the GPU resources — render
//! pipelines, glyph + gradient atlases, the image texture cache, the
//! device/queue — live on the **one** shared `WgpuBackend` the host passes
//! into every method; render caches, shaper, and the GPU-stats handle live on
//! the [`HostContext`]. The record store is per-window and retained alongside
//! this window's tree. So N windows render through one GPU renderer without
//! sharing frame-local geometry.
//!
//! Two public entries, sharing one CPU + GPU path:
//! [`WindowRenderer::frame`] (to a swapchain surface — acquires, submits,
//! presents, returns a [`FramePresent`] schedule) and
//! [`WindowRenderer::frame_offscreen`] (to a caller-supplied
//! `wgpu::Texture` — no acquire/present, for screenshots / the offscreen
//! host).

use std::time::Instant;

use crate::host::clock::{Clock, RealtimeClock};
use crate::host::context::HostContext;
use crate::record_store::RecordStore;
use crate::renderer::backend::{Backbuffer, Stencil, Submission, SubmissionTargets, WgpuBackend};
use crate::renderer::frontend::Frontend;
use crate::ui::Ui;
use crate::ui::damage::FULL_REPAINT_THRESHOLD;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::{Display, FrameReport, FrameStamp};

/// Per-window state driving the shared [`WgpuBackend`]. Built by
/// [`Self::new`] from the shared [`HostContext`]; owns no GPU resources
/// except its own [`Backbuffer`] + [`Stencil`].
#[derive(Debug)]
pub struct WindowRenderer {
    pub ui: Ui,
    /// Per-window record store retained in lockstep with `ui.forest`. The `Ui`
    /// holds an `Rc` clone for record-time writes; frontend and backend phases
    /// borrow this canonical handle explicitly.
    record_store: RecordStore,
    /// Per-window CPU encode/compose scratch with its own retained
    /// `RenderBuffer` — this window's draw list.
    frontend: Frontend,
    /// Persistent off-screen color target holding last frame's pixels for
    /// `LoadOp::Load` partial damage. Used by `BackbufferCopy` every frame and
    /// by `DirectAdaptive` for its small-partial path (paint the damage region,
    /// then copy out). A `DirectAdaptive` window that only ever paints full
    /// frames never allocates it. Created lazily on the first frame that needs
    /// it, recreated on resize / format change.
    backbuffer: Option<Backbuffer>,
    /// `true` when [`Self::backbuffer`] mirrors what's currently on the target
    /// (the last presented frame went through it), so a `DirectAdaptive` small
    /// partial can `LoadOp::Load` it and paint just the damage region. A direct
    /// full frame bypasses the backbuffer, leaving it stale (`false`) — the next
    /// partial then resyncs it with one full repaint before cheap partials
    /// resume. Irrelevant to `BackbufferCopy` (every frame touches the
    /// backbuffer, so it always stays fresh).
    backbuffer_fresh: bool,
    /// This window's rounded-clip stencil attachment — allocated lazily,
    /// resized to the target. Separate from `backbuffer` so the direct-present
    /// path can have a stencil without a backbuffer color texture.
    stencil: Option<Stencil>,
    /// How this window's frames reach the target — see [`PresentStrategy`].
    strategy: PresentStrategy,
    /// Per-frame time source — `clock.now()` feeds `Ui::frame` each call.
    /// Injected at construction ([`RealtimeClock`](crate::host::clock::RealtimeClock)
    /// for on-screen windows, [`FixedClock`](crate::host::clock::FixedClock) for a
    /// reproducible offscreen render) so the pipeline doesn't branch on it.
    clock: Box<dyn Clock>,
    /// `Some(instant the window went occluded)` while occluded — `frame()`
    /// short-circuits to `Idle` without running `cpu_frame`. Every
    /// per-frame Ui flag (damage, repaint_requested, animation driver
    /// state) is naturally preserved because nothing consumes it; input
    /// still flows through `Ui::on_input` and accumulates for the first
    /// un-occluded frame. On resume the clock is shifted forward by the
    /// elapsed hidden duration so anim drivers don't see a giant `dt`
    /// for the gap.
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
    /// flip changes nothing the `Ui` tracks — same size, same scene — so
    /// without noticing it here an unchanged scene would damage-`Skip`
    /// and copy the stale-format backbuffer. In practice only the
    /// offscreen path can flip (each `frame_offscreen` call brings its
    /// own `target.format()`); a swapchain's format is chosen once at
    /// surface creation and never rewritten. `frame` / `frame_offscreen`
    /// compare against it and force a full repaint on change (see
    /// [`Self::note_format`]). `None` until the first paint.
    last_format: Option<wgpu::TextureFormat>,
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PresentStrategy {
    /// A target whose prior contents can't be relied on — a fresh texture each
    /// call (screenshots, the visual harness). Every frame renders into the
    /// persistent backbuffer and copies out, so the whole target is filled
    /// regardless of its prior contents; skip frames copy the backbuffer.
    BackbufferCopy,
    /// A direct-present target — the swapchain (the host owns skip frames), or a
    /// reused offscreen texture. Full frames repaint straight into the target
    /// (no copy); small partials paint just the damage region into the
    /// backbuffer and copy it out (cheaper than repainting the whole surface);
    /// a near-full partial is promoted to a direct full repaint (its
    /// near-whole-surface re-shade plus a copy would beat a plain direct
    /// repaint). A direct frame leaves the backbuffer stale, so the next partial
    /// resyncs it with one full repaint before cheap partials resume. Skip
    /// frames are a no-op (the target already holds the last render).
    DirectAdaptive,
}

/// How a frame reaches the target, given its plan and the window's
/// [`PresentStrategy`]. Computed once in `cpu_frame` — which builds the
/// draw list for it — and threaded through to the GPU half, so the
/// submitted plan is by construction the one the draw list was built for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PresentMode {
    /// Skip frame on a backbuffer-copy window: copy the backbuffer onto the
    /// target so it's filled regardless of its prior contents.
    SkipCopy,
    /// Skip frame on a direct-present window: the target already holds the last
    /// render (or the host owns the skip), so there's nothing to do.
    SkipNoop,
    /// Full repaint rendered directly into the target — no backbuffer copy.
    Direct(RenderPlan),
    /// Render the plan into the backbuffer, then copy it onto the target.
    ViaBackbuffer(RenderPlan),
}

/// Coverage fraction above which [`PresentStrategy::DirectAdaptive`] promotes a
/// `Partial` to a direct full repaint instead of painting just the damage region
/// into the backbuffer and copying out. Read against the region's sealed
/// `coverage` — the same axis as the damage engine's [`FULL_REPAINT_THRESHOLD`],
/// and strictly below it (a promoted partial must still reach here *as* a
/// `Partial`, not already collapsed to `Full`; enforced by the assert below).
///
/// The backbuffer path pays a *fixed* whole-surface copy every frame regardless
/// of damage size, on top of re-shading every leaf the region intersects. Once a
/// partial touches enough geometry that its paint + copy approaches a plain full
/// repaint, going direct (which drops the copy) wins. Empirically the crossover
/// sits near 0.40 on the bandwidth-bound `frame` bench (Radeon 680M): the
/// `scrolling` arm shifts a panel transform so ~half the surface damages, yet the
/// band crosses dense scrolled content — 7.8 ms via backbuffer vs 6.8 ms direct.
/// Sub-threshold partials (the `partial` arm's footer counter is ~0.04 %) stay on
/// the backbuffer path, where a tiny re-shade + one copy (3.3 ms) beats a
/// whole-surface direct repaint (6.8 ms). Area is a proxy for paint cost, not a
/// measurement of it, so the line sits a little under the known-expensive scroll
/// band rather than at a precise break-even.
const DIRECT_PROMOTE_COVERAGE: f32 = 0.4;

// A promoted partial must still reach `present_mode` *as* a `Partial`, never
// collapsed to `Full` by the damage engine first — so the promote point stays
// strictly below `FULL_REPAINT_THRESHOLD`. Compile-time guard: retuning either
// past the other fails the build instead of silently killing promotion.
const _: () = assert!(DIRECT_PROMOTE_COVERAGE < FULL_REPAINT_THRESHOLD);

fn present_mode(
    plan: Option<RenderPlan>,
    strategy: PresentStrategy,
    backbuffer_fresh: bool,
) -> PresentMode {
    match strategy {
        PresentStrategy::DirectAdaptive => match plan {
            // Swapchain skips never reach here (the host owns them); a reused
            // offscreen target keeps its last render. Either way, nothing to do.
            None => PresentMode::SkipNoop,
            Some(p) => match p.kind {
                // Already a whole-surface repaint — straight into the target.
                RenderKind::Full => PresentMode::Direct(p),
                RenderKind::Partial { region } => {
                    // `region.coverage` was sealed when the damage engine built
                    // this region (`collapse_from`); see `DIRECT_PROMOTE_COVERAGE`.
                    if region.coverage > DIRECT_PROMOTE_COVERAGE {
                        // Large partial: skip the copy, repaint direct.
                        PresentMode::Direct(p.to_full())
                    } else if backbuffer_fresh {
                        // Backbuffer mirrors the target: paint just the damage
                        // region into it and copy out.
                        PresentMode::ViaBackbuffer(p)
                    } else {
                        // Backbuffer went stale after a direct frame: resync it
                        // with one full repaint before cheap partials resume.
                        PresentMode::ViaBackbuffer(p.to_full())
                    }
                }
            },
        },
        // Fresh target each call: render the plan into the backbuffer and copy
        // it out so the whole target is filled regardless of its prior contents.
        PresentStrategy::BackbufferCopy => match plan {
            None => PresentMode::SkipCopy,
            Some(p) => PresentMode::ViaBackbuffer(p),
        },
    }
}

/// The CPU half's result: the host-facing report plus the [`PresentMode`]
/// sealed at draw-list-build time. Threading the mode (rather than
/// recomputing it in the GPU half) is what guarantees the submitted plan
/// is the one the draw list was built for.
#[derive(Debug)]
struct CpuFrame {
    report: FrameReport,
    mode: PresentMode,
}

/// Builder for [`WindowRenderer`] — see [`WindowRenderer::builder`]. The
/// required inputs (`ctx`, `max_texture_dim`) come from that constructor;
/// `strategy` and `clock` start at the on-screen-window defaults.
pub(crate) struct WindowRendererBuilder<'a> {
    ctx: &'a HostContext,
    max_texture_dim: u32,
    strategy: PresentStrategy,
    clock: Box<dyn Clock>,
}

impl WindowRendererBuilder<'_> {
    /// Present strategy. Default [`PresentStrategy::DirectAdaptive`]; the
    /// offscreen host sets [`PresentStrategy::BackbufferCopy`] when it hands a
    /// fresh target each frame.
    pub(crate) fn strategy(mut self, strategy: PresentStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Per-frame time source. Default a wall-clock [`RealtimeClock`]; a
    /// [`FixedClock`](crate::host::clock::FixedClock) makes an offscreen render
    /// reproducible.
    pub(crate) fn clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub(crate) fn build(self) -> WindowRenderer {
        let record_store = RecordStore::default();
        WindowRenderer {
            ui: Ui::new(self.ctx, record_store.clone()),
            record_store,
            frontend: Frontend::new(self.max_texture_dim),
            backbuffer: None,
            backbuffer_fresh: false,
            stencil: None,
            strategy: self.strategy,
            clock: self.clock,
            occluded_at: None,
            configured: None,
            last_format: None,
        }
    }
}

impl WindowRenderer {
    /// Start building a per-window renderer from the shared [`HostContext`]:
    /// its `Ui` shares the context's shaper / caches / GPU-stats handle and
    /// receives a fresh per-window record store. The `Ui` also shares the context's
    /// app-global host state (live-window set + debug overlay) so all windows
    /// agree. `max_texture_dim` is the device's `max_texture_dimension_2d`
    /// (fixed for its lifetime), handed to the `Frontend` to cap `GpuView`
    /// target sizes — the only GPU fact the CPU pipeline needs.
    ///
    /// Defaults suit an on-screen window: [`PresentStrategy::DirectAdaptive`]
    /// and a wall-clock [`RealtimeClock`]. Override either via
    /// [`WindowRendererBuilder::strategy`] / [`WindowRendererBuilder::clock`]
    /// before [`WindowRendererBuilder::build`] (the offscreen host does both).
    pub(crate) fn builder(ctx: &HostContext, max_texture_dim: u32) -> WindowRendererBuilder<'_> {
        WindowRendererBuilder {
            ctx,
            max_texture_dim,
            strategy: PresentStrategy::DirectAdaptive,
            clock: Box::new(RealtimeClock::new()),
        }
    }

    /// Drive from the host's window-event handler. While occluded,
    /// `frame()` returns `Idle` without running CPU passes; pending
    /// Ui state (damage, repaint requests, animation deadlines)
    /// survives untouched until the window becomes visible again.
    pub fn set_occluded(&mut self, occluded: bool) {
        match (occluded, self.occluded_at) {
            (true, None) => self.occluded_at = Some(Instant::now()),
            (false, Some(t)) => {
                self.occluded_at = None;
                self.clock.skip(t.elapsed());
            }
            _ => {}
        }
    }

    /// Detect a color-format change against the last frame's target and,
    /// on change, force this frame to repaint fully. A format flip changes
    /// nothing the `Ui` tracks (same size, same scene), so an unchanged
    /// scene would otherwise damage-`Skip` and copy the stale-format
    /// backbuffer. Clearing `frame_submitted` forces a full record +
    /// clear (so submit, not the copy path, runs); the shared backend
    /// builds the new format's pipelines lazily and the backbuffer
    /// self-heals. Resetting `configured` forces a windowed surface
    /// reconfigure at the new format. Runs every frame — a no-op once
    /// the format is steady.
    fn note_format(&mut self, format: wgpu::TextureFormat) {
        if self.last_format != Some(format) {
            self.last_format = Some(format);
            self.ui.frame_runtime.frame_submitted = false;
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

        if self.occluded_at.is_some() {
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

        let cpu = self.cpu_frame(display, record);
        let present = self.present(gpu, surface, config, cpu, pre_present);

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
        let cpu = self.cpu_frame(display, record);
        self.render_to_texture(gpu, target, cpu.mode);
    }

    /// The CPU half of a frame: `Ui::frame` (record → measure / arrange /
    /// cascade / damage) followed, when the frame actually paints, by the
    /// draw-list build (encode → compose → resolve `GpuView`s into the
    /// frontend's buffer). Seals the [`PresentMode`] here — the one place
    /// it is computed — so the GPU half submits exactly the plan the draw
    /// list was built for (a promoted or resync'd Partial builds its
    /// escalated Full list). No GPU input — the `GpuView` size cap was
    /// captured on the `Frontend` at construction. Shared by
    /// [`Self::frame`] (surface) and [`Self::frame_offscreen`] (texture).
    #[profiling::function]
    fn cpu_frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) -> CpuFrame {
        let report = self
            .ui
            .frame(FrameStamp::new(display, self.clock.now()), record);
        let mode = present_mode(report.plan, self.strategy, self.backbuffer_fresh);
        // Build the draw list now (CPU) when the frame paints — encode,
        // compose, and resolve `GpuView` targets, all reading the now-frozen
        // `Ui` immutably. Skip frames build nothing.
        if let PresentMode::Direct(plan) | PresentMode::ViaBackbuffer(plan) = mode {
            let payloads = self.record_store.borrow();
            self.frontend.build(&self.ui, &payloads, plan);
        }
        CpuFrame { report, mode }
    }

    /// GPU submit against a caller-supplied texture, through the shared
    /// `gpu`, dispatching on the [`PresentMode`] `cpu_frame` sealed. On
    /// [`PresentMode::SkipCopy`], copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid pixels.
    /// Shared by [`Self::frame`]'s present path (the acquired swapchain
    /// texture) and [`Self::frame_offscreen`] (an offscreen texture).
    #[profiling::function]
    fn render_to_texture(
        &mut self,
        gpu: &mut WgpuBackend,
        target: &wgpu::Texture,
        mode: PresentMode,
    ) {
        let size = target.size();
        let display_phys = self.ui.display.physical;
        debug_assert!(
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
        // The CPU phase already composed `GpuView`s into
        // `self.frontend.buffer.frame_targets` (callback + size — see
        // `cpu_frame`); this is GPU submit only.
        let debug_overlay = self.ui.debug_overlay();
        // Rounded-clip stencil, shared by both paint paths and sized to the
        // target. Gated to them: on a skip frame the frontend didn't build,
        // so `buffer.rounded_clips` is stale and no pass reads the stencil.
        let stencil_view = match mode {
            PresentMode::Direct(_) | PresentMode::ViaBackbuffer(_)
                if !self.frontend.buffer.rounded_clips.is_empty() =>
            {
                gpu.ensure_stencil(&mut self.stencil, size);
                Some(&self.stencil.as_ref().expect("ensure_stencil ran").view)
            }
            _ => None,
        };
        // Skip arms leave `ui.frame_runtime.frame_submitted` alone —
        // `Ui::frame` acks a skip itself; the paint arms ack here after a
        // successful submit.
        match mode {
            // Nothing changed and the target already holds the last render —
            // leave it untouched.
            PresentMode::SkipNoop => {}
            PresentMode::SkipCopy => {
                // A `Skip` implies the previous frame painted at this size +
                // format, so the backbuffer must exist (and match — the
                // backend asserts that).
                let bb = self
                    .backbuffer
                    .as_ref()
                    .expect("SkipCopy implies a prior submitted paint frame");
                gpu.copy_backbuffer_to_surface(bb, target);
            }
            // Full repaint straight into the target — no backbuffer at all, so
            // it goes stale: the next partial must resync it first.
            PresentMode::Direct(plan) => {
                let payloads = self.record_store.borrow();
                gpu.submit(Submission {
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: None,
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer: &self.frontend.buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = false;
                self.ui.frame_runtime.frame_submitted = true;
            }
            // Render into the backbuffer and copy it out; the backbuffer now
            // mirrors the target.
            PresentMode::ViaBackbuffer(plan) => {
                let recreated = gpu.ensure_backbuffer(&mut self.backbuffer, size, target.format());
                // A Partial reaches here un-escalated only when
                // `backbuffer_fresh` — last frame rendered into the backbuffer
                // at this size/format — so a recreate under Partial means the
                // freshness invariant broke. Escalating here couldn't fix it:
                // the draw list was already Partial-culled in `cpu_frame`.
                debug_assert!(
                    !recreated || matches!(plan.kind, RenderKind::Full),
                    "backbuffer (re)created under a Partial plan whose draw \
                     list was culled for Partial"
                );
                let payloads = self.record_store.borrow();
                gpu.submit(Submission {
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: self.backbuffer.as_ref(),
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer: &self.frontend.buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = true;
                self.ui.frame_runtime.frame_submitted = true;
            }
        }
    }

    #[profiling::function]
    fn present(
        &mut self,
        gpu: &mut WgpuBackend,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        cpu: CpuFrame,
        pre_present: impl FnOnce(),
    ) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        let repaint = if report.plan.is_none() {
            report.repaint_requested
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            match surface.get_current_texture() {
                Success(frame) => {
                    self.render_to_texture(gpu, &frame.texture, mode);
                    // Compositor hook (winit's `Window::pre_present_notify`)
                    // — required on Wayland to schedule the next frame
                    // callback. Without it, `RedrawRequested` throttling
                    // breaks and interactive resize / animation lag
                    // behind the compositor's configure cadence. See
                    // winit #2609, slint #4200.
                    pre_present();
                    gpu.present(frame);
                    report.repaint_requested
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
        } else if let Some(at) = report.repaint_after.and_then(|d| self.clock.deadline(d)) {
            FramePresent::At(at)
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

#[cfg(test)]
mod present_mode_tests {
    use super::PresentMode::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use super::PresentStrategy::{BackbufferCopy, DirectAdaptive};
    use super::{PresentMode, present_mode};
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::ui::frame_report::{RenderKind, RenderPlan};

    /// 100×100 logical surface (10_000 px²) the partial fixtures collapse
    /// against, so a `w×h` damage rect carries `coverage = w·h / 10_000`.
    const SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

    fn full() -> Option<RenderPlan> {
        Some(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Full,
        })
    }
    /// One `Rect` of `w·h` px², built through `collapse_from` against
    /// [`SURFACE`] so its `region.coverage` is `w·h / 10_000` — exactly what the
    /// damage engine seals in the real path.
    fn partial(w: f32, h: f32) -> Option<RenderPlan> {
        let region = DamageRegion::collapse_from(
            &[Rect::new(0.0, 0.0, w, h)],
            DEFAULT_PASS_BUDGET_PX,
            SURFACE,
        );
        Some(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Partial { region },
        })
    }
    const DIRECT_FULL: PresentMode = Direct(RenderPlan {
        clear: Color::BLACK,
        kind: RenderKind::Full,
    });

    #[test]
    fn backbuffer_copy_fills_target_through_backbuffer() {
        // Fresh target each call: paint via the backbuffer (the requested plan,
        // Full or Partial), skip copies it out — the whole target is filled.
        // Backbuffer freshness is irrelevant here (every frame touches it).
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), BackbufferCopy, fresh),
                ViaBackbuffer(full().unwrap())
            );
            assert_eq!(
                present_mode(partial(10.0, 10.0), BackbufferCopy, fresh),
                ViaBackbuffer(partial(10.0, 10.0).unwrap())
            );
            assert_eq!(present_mode(None, BackbufferCopy, fresh), SkipCopy);
        }
    }

    #[test]
    fn direct_adaptive_full_and_skip() {
        // A whole-surface repaint goes straight in; a skip is a noop. Neither
        // depends on backbuffer freshness.
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), DirectAdaptive, fresh),
                Direct(full().unwrap())
            );
            assert_eq!(present_mode(None, DirectAdaptive, fresh), SkipNoop);
        }
    }

    #[test]
    fn direct_adaptive_small_partial_tracks_backbuffer_freshness() {
        // 10×10 = 100 px² ⇒ coverage 0.01, well under the 0.4 promote line.
        let small = partial(10.0, 10.0);
        // Fresh: the backbuffer mirrors the target, so paint just the region.
        assert_eq!(
            present_mode(small, DirectAdaptive, true),
            ViaBackbuffer(small.unwrap())
        );
        // Stale (after a direct frame): resync with one full repaint first.
        assert_eq!(
            present_mode(small, DirectAdaptive, false),
            ViaBackbuffer(full().unwrap())
        );
    }

    #[test]
    fn direct_adaptive_large_partial_promotes_to_direct() {
        // 80×80 = 6_400 px² ⇒ coverage 0.64 > 0.4: a large partial repaints
        // direct (dropping the copy) regardless of backbuffer freshness.
        let large = partial(80.0, 80.0);
        for fresh in [false, true] {
            assert_eq!(present_mode(large, DirectAdaptive, fresh), DIRECT_FULL);
        }
    }

    #[test]
    fn direct_adaptive_promote_threshold_is_strict() {
        // Coverage at-or-below 0.4 stays on the backbuffer path (`>`, not `>=`);
        // just over promotes. 63×63 = 3_969 (0.3969) vs 64×64 = 4_096 (0.4096) —
        // straddling the 0.4 line.
        assert!(matches!(
            present_mode(partial(63.0, 63.0), DirectAdaptive, true),
            ViaBackbuffer(_)
        ));
        assert_eq!(
            present_mode(partial(64.0, 64.0), DirectAdaptive, true),
            DIRECT_FULL
        );
    }
}

#[cfg(test)]
mod record_store_tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::host::clock::FixedClock;
    use crate::host::context::HostContext;
    use crate::host::window_renderer::WindowRenderer;
    use crate::primitives::color::{Color, ColorU8};
    use crate::primitives::mesh::{Mesh, MeshVertex};
    use crate::primitives::widget_id::WidgetId;
    use crate::shape::{PolylineColors, Shape};
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameProcessing;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;
    use crate::{Configure, Display};

    #[derive(Debug, PartialEq)]
    struct RecordPayloadSnapshot {
        mesh_vertices: Vec<MeshVertex>,
        mesh_indices: Vec<u32>,
        polyline_points: Vec<Vec2>,
        polyline_colors: Vec<ColorU8>,
        text: String,
    }

    fn snapshot(renderer: &WindowRenderer) -> RecordPayloadSnapshot {
        let payloads = renderer.record_store.borrow();
        RecordPayloadSnapshot {
            mesh_vertices: payloads.meshes.vertices.clone(),
            mesh_indices: payloads.meshes.indices.clone(),
            polyline_points: payloads.polyline_points.clone(),
            polyline_colors: payloads.polyline_colors.clone(),
            text: payloads.fmt_scratch.clone(),
        }
    }

    fn record_scene(
        ui: &mut Ui,
        mesh: &Mesh,
        points: &[Vec2],
        colors: &[Color],
        label: &str,
        id: &'static str,
    ) {
        Panel::zstack()
            .id(WidgetId::from_hash(id))
            .size(96.0)
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(mesh));
                ui.add_shape(Shape::polyline(
                    points,
                    PolylineColors::PerPoint(colors),
                    3.0,
                ));
                let label = ui.intern(label);
                Text::new(label)
                    .id(WidgetId::from_hash((id, "text")))
                    .show(ui);
                Spinner::new()
                    .id(WidgetId::from_hash((id, "spinner")))
                    .size(92.0)
                    .show(ui);
            });
    }

    /// A record pass in one window must not replace the payloads retained by
    /// another window's animation-only frame.
    #[test]
    fn interleaved_window_paint_only_preserves_record_payloads() {
        let ctx = HostContext::new(TextShaper::default());
        let mut window_a = WindowRenderer::builder(&ctx, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let mut window_b = WindowRenderer::builder(&ctx, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let display = Display::from_physical(UVec2::new(112, 112), 1.0);

        let mesh_a = Mesh::filled_triangle(
            Vec2::new(12.0, 14.0),
            Vec2::new(72.0, 20.0),
            Vec2::new(26.0, 74.0),
            Color::rgb(0.15, 0.65, 0.95),
        );
        let points_a = [
            Vec2::new(8.0, 82.0),
            Vec2::new(28.0, 10.0),
            Vec2::new(68.0, 84.0),
            Vec2::new(88.0, 12.0),
        ];
        let colors_a = [
            Color::rgb(1.0, 0.0, 0.0),
            Color::WHITE,
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(0.0, 0.0, 1.0),
        ];

        let mesh_b = Mesh::filled_polygon(
            &[
                Vec2::new(78.0, 8.0),
                Vec2::new(90.0, 46.0),
                Vec2::new(58.0, 88.0),
                Vec2::new(14.0, 70.0),
                Vec2::new(8.0, 24.0),
            ],
            Color::rgb(0.9, 0.2, 0.65),
        );
        let points_b = [
            Vec2::new(90.0, 88.0),
            Vec2::new(82.0, 18.0),
            Vec2::new(58.0, 64.0),
            Vec2::new(38.0, 14.0),
            Vec2::new(20.0, 76.0),
            Vec2::new(6.0, 32.0),
        ];
        let colors_b = [
            Color::WHITE,
            Color::rgb(0.0, 0.0, 1.0),
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(1.0, 0.0, 0.0),
            Color::BLACK,
            Color::WHITE,
        ];

        let _ = window_a.cpu_frame(display, |ui| {
            record_scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
        });
        window_a.ui.frame_runtime.frame_submitted = true;
        let retained = snapshot(&window_a);
        assert_eq!(retained.mesh_vertices.len(), 3);
        assert_eq!(retained.polyline_points.len(), 4);
        assert_eq!(retained.text, "retained A");

        let _ = window_b.cpu_frame(display, |ui| {
            record_scene(
                ui,
                &mesh_b,
                &points_b,
                &colors_b,
                "window B has a much longer label",
                "window-b",
            );
        });
        window_b.ui.frame_runtime.frame_submitted = true;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.cpu_frame(display, |ui| {
            record_scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
        });
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}
