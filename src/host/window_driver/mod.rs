//! `WindowDriver` — the target-agnostic state one host target owns around the
//! shared renderer: its [`Ui`] recorder, stable render-stream identity, the
//! persistent [`Backbuffer`] (the target's last-frame pixels), and the
//! per-target frame clock.
//!
//! What every window shares splits two ways: CPU encode/compose scratch lives
//! on the one [`Frontend`] each host passes into the frame methods; GPU resources — render
//! pipelines, glyph + gradient atlases, the image texture cache, and renderer
//! device/queue handles — live on the **one** shared `WgpuBackend` the host
//! passes into every method; renderer and recorder capabilities derive from
//! [`HostShared`]. Each `Ui` owns
//! its per-window record store alongside its tree. So N windows render through
//! one GPU renderer without sharing frame-local geometry.
//!
//! [`WindowDriver::cpu_frame`] freezes the frame and builds the draw list into
//! the shared frontend; [`WindowDriver::render_to_texture`] submits it to any caller-owned texture.
//! [`crate::OffscreenHost`] drives those operations directly, while the winit
//! adapter owns swapchain acquisition, presentation, occlusion, and wake
//! scheduling.

use glam::UVec2;

use crate::app::App;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::shared::HostShared;
use crate::renderer::backend::WgpuBackend;
use crate::renderer::backend::backbuffer::Backbuffer;
use crate::renderer::backend::stencil::Stencil;
use crate::renderer::backend::submission::Submission;
use crate::renderer::backend::submission::SubmissionTargets;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::{RenderKind, RenderPlan};
use crate::scene::damage::FULL_REPAINT_THRESHOLD;
use crate::ui::Ui;
use crate::ui::frame_input::FrameInput;
use crate::ui::frame_stamp::FrameStamp;
use crate::window::window_commands::WindowCommands;
use crate::window::window_frame_state::WindowFrameState;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;
use crate::{Display, FrameReport};

/// Per-window state driving the host's shared [`Frontend`] and [`WgpuBackend`].
/// Built by [`WindowDriverBuilder`] from the shared [`HostShared`]; owns no GPU
/// resources except its own [`Backbuffer`] + [`Stencil`].
#[derive(Debug)]
pub(super) struct WindowDriver {
    /// Stable application identity for this render stream. Stored here so a
    /// retained `Ui` cannot be driven under a different token on a later frame.
    pub(super) token: WindowToken,
    pub(super) ui: Ui,
    /// Stable submitter identity used by the shared backend to scope retained
    /// `GpuView` targets to this window.
    pub(super) render_owner: RenderOwnerId,
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
    /// Whether the last frame completed the presentation action selected by
    /// this driver. Invalid while a paint/copy is pending or after target
    /// invalidation, so the next UI frame discards its prior damage baseline.
    output_valid: bool,
    /// This window's rounded-clip stencil attachment — allocated lazily,
    /// resized to the target. Separate from `backbuffer` so the direct-present
    /// path can have a stencil without a backbuffer color texture.
    stencil: Option<Stencil>,
    /// How this window's frames reach the target — see [`PresentStrategy`].
    strategy: PresentStrategy,
    /// Per-frame time source — `clock.now()` feeds `Ui::frame` each call.
    /// Injected at construction ([`RealtimeClock`] for on-screen windows,
    /// [`FixedClock`](crate::host::clock::FixedClock) for a reproducible
    /// offscreen render) so the pipeline doesn't branch on it.
    pub(super) clock: Box<dyn Clock>,
    /// Whether axis-aligned paint edges snap to physical pixels. Reaches
    /// a frame only through [`Self::display`].
    pixel_snap: bool,
    /// The target the last [`Self::note_target`] saw. `None` until the first
    /// frame; a mismatch is what invalidates the retained target state.
    target: Option<TargetKey>,
}

/// Identity of the surface or texture a window renders into: everything a
/// change of which invalidates the driver's retained target state (last-frame
/// pixels, damage baseline, backbuffer format).
///
/// This is the **single gate** on that state, and on a swapchain host it is
/// also what decides when to reconfigure the surface. Any
/// `wgpu::SurfaceConfiguration` field that becomes mutable at runtime must
/// therefore be added here, or the new value sits in the host's config and
/// silently never reaches the swapchain — nothing else re-reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TargetKey {
    pub(super) physical: UVec2,
    pub(super) format: wgpu::TextureFormat,
    /// `None` for a plain texture target, which is never presented and so
    /// has no swapchain to reconfigure.
    pub(super) present_mode: Option<wgpu::PresentMode>,
}

impl TargetKey {
    pub(super) fn of(texture: &wgpu::Texture) -> Self {
        let size = texture.size();
        Self {
            physical: UVec2::new(size.width, size.height),
            format: texture.format(),
            present_mode: None,
        }
    }

    /// Whether this key describes a target with the given texture facts.
    ///
    /// Compares only what a `wgpu::Texture` can answer for. `present_mode` is
    /// a *swapchain* property, so a surface key legitimately holds `Some(..)`
    /// while the frame's acquired texture — an ordinary texture — carries no
    /// trace of it. Equality would therefore never hold on a swapchain host;
    /// this is the predicate for "same target", as opposed to [`PartialEq`]'s
    /// "same target *configuration*", which is what [`WindowDriver::note_target`]
    /// gates invalidation and surface reconfiguration on.
    pub(super) fn describes(&self, physical: UVec2, format: wgpu::TextureFormat) -> bool {
        self.physical == physical && self.format == format
    }
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PresentStrategy {
    /// A target whose prior contents can't be relied on — a fresh texture each
    /// call (screenshots, the visual harness). Every frame renders into the
    /// persistent backbuffer and copies out, so the whole target is filled
    /// regardless of its prior contents; skip frames copy the backbuffer.
    BackbufferCopy,
    /// A direct-present swapchain target, where the host owns skip frames. Full
    /// frames repaint straight into the target (no copy); small partials paint
    /// just the damage region into the backbuffer and copy it out (cheaper than
    /// repainting the whole surface); a near-full partial is promoted to a
    /// direct full repaint. A direct frame leaves the backbuffer stale, so the
    /// next partial resyncs it with one full repaint before cheap partials
    /// resume.
    DirectAdaptive,
}

/// How a frame reaches the target, given its plan and the window's
/// [`PresentStrategy`]. Computed once in `cpu_frame` — which builds the
/// draw list for it — and threaded through to the GPU half, so the
/// submitted plan is by construction the one the draw list was built for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PresentMode {
    /// Skip frame on a backbuffer-copy window: copy the backbuffer onto the
    /// target so it's filled regardless of its prior contents.
    SkipCopy,
    /// Skip frame on a direct-present window: the host owns the skip, so there
    /// is no target to update.
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
            // Swapchain skips never acquire a target because the host owns them.
            None => PresentMode::SkipNoop,
            Some(p) => match p.kind {
                // Already a whole-surface repaint — straight into the target.
                RenderKind::Full => PresentMode::Direct(p),
                RenderKind::Partial { damage } => {
                    // The coverage the damage engine measured when it
                    // collapsed this frame's rects against the surface; see
                    // `DIRECT_PROMOTE_COVERAGE`.
                    if damage.coverage > DIRECT_PROMOTE_COVERAGE {
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
pub(super) struct CpuFrame {
    pub(super) report: FrameReport,
    pub(super) mode: PresentMode,
}

/// Seals per-window policy before allocating the recorder.
#[derive(Debug)]
pub(super) struct WindowDriverBuilder<'a> {
    token: WindowToken,
    shared: &'a HostShared,
    strategy: PresentStrategy,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
}

impl WindowDriverBuilder<'_> {
    pub(super) fn strategy(mut self, strategy: PresentStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub(super) fn clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub(super) fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    pub(super) fn build(self) -> WindowDriver {
        WindowDriver {
            token: self.token,
            ui: Ui::new(self.shared.resources.clone()),
            render_owner: RenderOwnerId::reserve(),
            backbuffer: None,
            backbuffer_fresh: false,
            output_valid: false,
            stencil: None,
            strategy: self.strategy,
            clock: self.clock,
            pixel_snap: self.pixel_snap,
            target: None,
        }
    }
}

impl WindowDriver {
    /// Start building a driver for `token` from the shared [`HostShared`].
    /// Its `Ui` receives recorder capabilities plus a fresh per-window record
    /// store. Defaults suit a swapchain window: direct adaptive presentation,
    /// realtime clock, and physical-pixel snapping.
    pub(super) fn builder(token: WindowToken, shared: &HostShared) -> WindowDriverBuilder<'_> {
        WindowDriverBuilder {
            token,
            shared,
            strategy: PresentStrategy::DirectAdaptive,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap: true,
        }
    }

    /// This driver's [`Display`] for a frame of the given surface.
    ///
    /// **The one place `pixel_snap` reaches a frame.** It is the driver's,
    /// fixed by the builder — but `Display`'s own default is `true`, so a
    /// host that assembled one itself would snap regardless of what it
    /// asked for, and nothing would say so. Minting it here leaves
    /// nothing to remember: a caller supplies what it knows (the surface,
    /// and the monitor's refresh where it has one) and the driver
    /// supplies what it owns.
    pub(super) fn display(
        &self,
        physical: UVec2,
        scale_factor: f32,
        refresh_millihertz: Option<u32>,
    ) -> Display {
        Display {
            physical,
            scale_factor,
            pixel_snap: self.pixel_snap,
            refresh_millihertz,
        }
    }

    /// Declare the target the next [`Self::cpu_frame`] renders into, returning
    /// whether it differs from the last one. A change invalidates every piece
    /// of state whose correctness depends on the target — retained pixels and
    /// the damage baseline — so the next frame repaints in full. Target-owning
    /// adapters call this before each CPU frame; a swapchain adapter also
    /// reconfigures its surface on a `true`.
    pub(super) fn note_target(&mut self, key: TargetKey) -> bool {
        if self.target == Some(key) {
            return false;
        }
        self.target = Some(key);
        self.output_valid = false;
        self.backbuffer_fresh = false;
        true
    }

    /// The shared CPU half: app lifecycle → record / measure / arrange /
    /// cascade / damage followed, when the frame actually paints, by the
    /// draw-list build (encode → compose → resolve `GpuView`s into the
    /// frontend's buffer). Seals the [`PresentMode`] here — the one place it
    /// is computed — so the GPU half submits exactly the plan the draw list
    /// was built for (a promoted or resync'd Partial builds its escalated Full
    /// list). No GPU input — the `GpuView` size cap was captured on the
    /// `Frontend` at construction. Shared by the offscreen and surface
    /// adapters.
    #[profiling::function]
    pub(super) fn cpu_frame<T: App>(
        &mut self,
        frontend: &mut Frontend,
        display: Display,
        app: &mut T,
    ) -> CpuFrame {
        let report = self.ui.frame(
            FrameInput {
                stamp: FrameStamp::new(display, self.clock.now()),
                damage_baseline_valid: self.output_valid,
            },
            self.token,
            app,
        );
        self.finish_cpu_frame(frontend, report)
    }

    fn finish_cpu_frame(&mut self, frontend: &mut Frontend, report: FrameReport) -> CpuFrame {
        let mode = present_mode(report.plan, self.strategy, self.backbuffer_fresh);
        if !matches!(mode, PresentMode::SkipNoop) {
            self.output_valid = false;
        }
        // Build the draw list now (CPU) when the frame paints — encode,
        // compose, and resolve `GpuView` targets from the frozen scene.
        // Skip frames build nothing.
        if let PresentMode::Direct(plan) | PresentMode::ViaBackbuffer(plan) = mode {
            frontend.build(self.ui.frame_scene(), plan);
        }
        CpuFrame { report, mode }
    }

    /// Drain the recorder's post-frame window scratch into `commands` and
    /// return what the host applies afterwards. Settles the pending close
    /// first: a close request app code did not veto becomes this window's own
    /// close command, so every host applies the veto the same way. Shared by
    /// the offscreen and surface adapters — the offscreen one drains into a
    /// scratch buffer it then drops, since a headless render has no window
    /// lifecycle to service.
    ///
    /// The vsync setting is copied, not taken: it is a level the recorder
    /// keeps (and reads back through `Ui::vsync`), so the host is the one
    /// that diffs it against the swapchain it has open.
    ///
    /// Uses `Vec::append` rather than `mem::take` so the recorder keeps its
    /// buffers' capacity across frames.
    ///
    /// Reached in every build: the windowed host drains through it, and so —
    /// via [`Self::deny_window_commands`] — does the offscreen one.
    pub(super) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        let requests = &mut self.ui.window_requests;
        if self.ui.window_frame.close_requested && !requests.close_vetoed {
            requests.commands.closes.push(self.token);
        }
        commands.append(&mut requests.commands);
        // The veto lives exactly one frame, and this is where that is
        // enforced — for both hosts, since the offscreen counterpart
        // below drains through here too. Every winit frame reaches this,
        // occluded ones included, so no caller needs to clear it on the
        // way in.
        requests.close_vetoed = false;
        self.ui.window_frame = WindowFrameState::default();
        requests.levels
    }

    /// [`Self::drain_window_output`] for a host with no window lifecycle:
    /// drain the same way, then reject the half of the output nothing here
    /// can service.
    ///
    /// **The split does the deciding, not a per-field choice.** The
    /// *levels* — cursor and vsync — are settings the recorder retains and
    /// reads back through `Ui`, so a host with no window to apply them to
    /// drops its copy and leaves the app's own view of them intact; they
    /// are inert here, not lost. The *commands* — open and close — are
    /// edges that mean nothing unless something services them, and
    /// swallowing one leaves the app believing a window appeared. So they
    /// are a caller error rather than a no-op, and an app can tell which
    /// of its window calls this host honours by which half the call is in.
    ///
    /// # Panics
    ///
    /// Panics if this frame recorded any window open or close request.
    pub(super) fn deny_window_commands(&mut self) {
        // Empty by the contract below, so the two `append`s inside the
        // drain move nothing and this allocates on no path that returns.
        let mut denied = WindowCommands::default();
        self.drain_window_output(&mut denied);
        assert!(
            denied.opens.is_empty(),
            "Ui::open_window({:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — use \
             WinitHost if the app needs to open windows",
            denied.opens[0].token
        );
        assert!(
            denied.closes.is_empty(),
            "Ui::close_window({:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — drop the \
             host to release it",
            denied.closes[0]
        );
    }

    /// GPU submit against a caller-supplied texture, through the shared
    /// `backend`, dispatching on the [`PresentMode`] `cpu_frame` sealed. On
    /// [`PresentMode::SkipCopy`], copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid pixels.
    /// Shared by the offscreen and surface adapters.
    #[profiling::function]
    pub(super) fn render_to_texture(
        &mut self,
        buffer: &RenderBuffer,
        backend: &mut WgpuBackend,
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
        debug_assert!(
            self.target.is_some_and(|key| {
                key.describes(UVec2::new(size.width, size.height), target.format())
            }),
            "render_to_texture: target ({}x{}, {:?}) differs from the one \
             `note_target` declared ({:?}), so the retained backbuffer / damage \
             baseline were never invalidated for it",
            size.width,
            size.height,
            target.format(),
            self.target,
        );
        // The CPU phase already composed `GpuView`s into
        // `buffer.frame_targets` (callback + raster target — see
        // `cpu_frame`); this is GPU submit only.
        let debug_overlay = *self.ui.resources.diagnostics.overlay.borrow();
        // Rounded-clip stencil, shared by both paint paths and sized to the
        // target. Gated to them: on a skip frame the frontend didn't build,
        // so `buffer.rounded_clips` is stale and no pass reads the stencil.
        let stencil_view = match mode {
            PresentMode::Direct(_) | PresentMode::ViaBackbuffer(_)
                if !buffer.rounded_clips.is_empty() =>
            {
                Some(&backend.ensure_stencil(&mut self.stencil, size).view)
            }
            _ => None,
        };
        match mode {
            // Nothing changed and the target already holds the last render —
            // leave it untouched.
            PresentMode::SkipNoop => self.output_valid = true,
            PresentMode::SkipCopy => {
                // A `Skip` implies the previous frame painted at this size +
                // format, so the backbuffer must exist (and match — the
                // backend asserts that).
                let bb = self
                    .backbuffer
                    .as_ref()
                    .expect("SkipCopy implies a prior submitted paint frame");
                backend.copy_backbuffer_to_surface(bb, target);
                self.output_valid = true;
            }
            // Full repaint straight into the target — no backbuffer at all, so
            // it goes stale: the next partial must resync it first.
            PresentMode::Direct(plan) => {
                let payloads = self.ui.forest.record_store.payloads.borrow();
                backend.submit(Submission {
                    owner: self.render_owner,
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: None,
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = false;
                self.output_valid = true;
            }
            // Render into the backbuffer and copy it out; the backbuffer now
            // mirrors the target.
            PresentMode::ViaBackbuffer(plan) => {
                let recreated =
                    backend.ensure_backbuffer(&mut self.backbuffer, size, target.format());
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
                let payloads = self.ui.forest.record_store.payloads.borrow();
                backend.submit(Submission {
                    owner: self.render_owner,
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: self.backbuffer.as_ref(),
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = true;
                self.output_valid = true;
            }
        }
    }
}

#[cfg(test)]
mod tests;
