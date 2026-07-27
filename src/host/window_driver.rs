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
use crate::renderer::backend::{Backbuffer, Stencil, Submission, SubmissionTargets, WgpuBackend};
use crate::renderer::frontend::Frontend;
use crate::renderer::plan::{RenderKind, RenderPlan};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner::RenderOwnerId;
use crate::scene::damage::FULL_REPAINT_THRESHOLD;
use crate::ui::Ui;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::window::{WindowCommands, WindowFrameState, WindowOutput, WindowToken};
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
    /// Injected at construction ([`RealtimeClock`](crate::host::clock::RealtimeClock)
    /// for on-screen windows, [`FixedClock`](crate::host::clock::FixedClock) for a
    /// reproducible offscreen render) so the pipeline doesn't branch on it.
    pub(super) clock: Box<dyn Clock>,
    /// Whether axis-aligned paint edges snap to physical pixels.
    pub(super) pixel_snap: bool,
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
    /// The vsync request is **taken**, not copied: it is a one-shot ask, so
    /// leaving it set would re-apply the same swapchain reconfigure every
    /// frame.
    ///
    /// Uses `Vec::append` rather than `mem::take` so the recorder keeps its
    /// buffers' capacity across frames.
    #[cfg_attr(
        not(feature = "winit-host"),
        expect(
            dead_code,
            reason = "multi-window lifecycle plumbing: every caller is under                       src/host/winit/, so a build without that feature has                       nothing to call it"
        )
    )]
    pub(super) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        let requests = &mut self.ui.window_requests;
        if self.ui.window_frame.close_requested && !requests.close_vetoed {
            requests.commands.closes.push(self.token);
        }
        commands.append(&mut requests.commands);
        requests.close_vetoed = false;
        self.ui.window_frame = WindowFrameState::default();
        WindowOutput {
            cursor: requests.cursor,
            vsync: requests.vsync.take(),
        }
    }

    /// The counterpart to [`Self::drain_window_output`] for a host with no
    /// window lifecycle. Nothing there can service an open or close, and
    /// silently dropping one hides a real mistake — the app believes a window
    /// appeared — so a recorded request is a caller error, not a no-op.
    ///
    /// # Panics
    ///
    /// Panics if this frame recorded any window open or close request.
    pub(super) fn deny_window_requests(&mut self) {
        let commands = &self.ui.window_requests.commands;
        assert!(
            commands.opens.is_empty(),
            "Ui::open_window({:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — use \
             WinitHost if the app needs to open windows",
            commands.opens[0].token
        );
        assert!(
            commands.closes.is_empty(),
            "Ui::close_window({:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — drop the \
             host to release it",
            commands.closes[0]
        );
        // `keep_open` vetoes a close this host never requests; clear it so the
        // flag can't carry across frames.
        self.ui.window_requests.close_vetoed = false;
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
        debug_assert_eq!(
            self.target,
            Some(TargetKey::of(target)),
            "render_to_texture: target differs from the one `note_target` \
             declared, so the retained backbuffer / damage baseline were never \
             invalidated for it"
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
                backend.ensure_stencil(&mut self.stencil, size);
                Some(&self.stencil.as_ref().expect("ensure_stencil ran").view)
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
mod present_mode_tests {
    use crate::host::window_driver::PresentMode::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use crate::host::window_driver::PresentStrategy::{BackbufferCopy, DirectAdaptive};
    use crate::host::window_driver::{PresentMode, present_mode};
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};

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
mod output_validity_tests {
    use glam::UVec2;

    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentMode, PresentStrategy, TargetKey, WindowDriver};
    use crate::primitives::color::Color;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::text::TextShaper;
    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use crate::window::{WindowConfig, WindowToken};

    fn driver(token: WindowToken, shared: &HostShared) -> WindowDriver {
        WindowDriver::builder(token, shared).build()
    }

    /// A host with no window lifecycle refuses the request instead of dropping
    /// it, and clears the veto flag a `keep_open` may have left behind.
    #[test]
    fn deny_window_requests_accepts_a_quiet_frame_and_clears_the_veto() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut quiet = driver(WindowToken(1), &shared);
        quiet.ui.keep_open();

        quiet.deny_window_requests();

        assert!(
            !quiet.ui.window_requests.close_vetoed,
            "a veto against a close that was never requested must not persist"
        );
    }

    #[test]
    #[should_panic(expected = "Ui::open_window(WindowToken(9))")]
    fn deny_window_requests_rejects_an_open() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut opener = driver(WindowToken(1), &shared);
        opener
            .ui
            .open_window(WindowToken(9), WindowConfig::new("unservable"));

        opener.deny_window_requests();
    }

    #[test]
    #[should_panic(expected = "Ui::close_window(WindowToken(4))")]
    fn deny_window_requests_rejects_a_close() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut closer = driver(WindowToken(1), &shared);
        closer.ui.close_window(WindowToken(4));

        closer.deny_window_requests();
    }

    fn report(plan: Option<RenderPlan>) -> FrameReport {
        FrameReport {
            repaint_requested: false,
            repaint_after: None,
            plan,
            processing: FrameProcessing::SingleLayout,
        }
    }

    /// `note_target` is the single gate on retained target state: it reports a
    /// change exactly once per distinct size/format/present-mode, and every
    /// change clears the last-frame pixels and the damage baseline.
    ///
    /// The present-mode axis is what a runtime vsync toggle rides: applying
    /// one only rewrites the host's `SurfaceConfiguration`, and this gate is
    /// the sole thing that re-reads it, so a key blind to the field would
    /// leave the swapchain on the old mode forever.
    #[test]
    fn note_target_tracks_size_format_and_present_mode_and_invalidates_on_change() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut driver = WindowDriver::builder(WindowToken(1), &shared).build();
        let first = TargetKey {
            physical: UVec2::new(64, 48),
            format: wgpu::TextureFormat::Rgba8Unorm,
            present_mode: Some(wgpu::PresentMode::AutoVsync),
        };
        let resized = TargetKey {
            physical: UVec2::new(65, 48),
            ..first
        };
        let reformatted = TargetKey {
            format: wgpu::TextureFormat::Bgra8Unorm,
            ..resized
        };
        let vsync_off = TargetKey {
            present_mode: Some(wgpu::PresentMode::AutoNoVsync),
            ..reformatted
        };
        // A texture target is never presented, so it carries no mode at all —
        // and must still read as a change against an otherwise-equal surface.
        let offscreen = TargetKey {
            present_mode: None,
            ..vsync_off
        };

        assert!(driver.note_target(first), "the first target is a change");
        assert!(!driver.note_target(first), "an identical target is not");

        for changed in [resized, reformatted, vsync_off, offscreen] {
            driver.output_valid = true;
            driver.backbuffer_fresh = true;
            assert!(driver.note_target(changed));
            assert!(!driver.output_valid, "target change invalidates output");
            assert!(
                !driver.backbuffer_fresh,
                "target change invalidates retained target pixels"
            );
            assert!(!driver.note_target(changed));
        }

        // Repeats after a change must not re-invalidate: a swapchain window
        // paints every frame against a steady target and would never keep a
        // damage baseline if they did.
        driver.output_valid = true;
        driver.backbuffer_fresh = true;
        assert!(!driver.note_target(offscreen));
        assert!(driver.output_valid);
        assert!(driver.backbuffer_fresh);
    }

    #[test]
    fn window_drivers_have_distinct_render_owners() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let first = WindowDriver::builder(WindowToken(1), &shared).build();
        let second = WindowDriver::builder(WindowToken(2), &shared).build();

        assert_ne!(first.render_owner, second.render_owner);
    }

    #[test]
    fn output_validity_tracks_pending_and_completion() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
        let mut driver = WindowDriver::builder(WindowToken(1), &shared).build();
        assert!(!driver.output_valid, "first frame has no presented output");

        driver.output_valid = true;
        let paint = driver.finish_cpu_frame(
            &mut frontend,
            report(Some(RenderPlan {
                clear: Color::BLACK,
                kind: RenderKind::Full,
            })),
        );
        assert!(matches!(paint.mode, PresentMode::Direct(_)));
        assert!(
            !driver.output_valid,
            "paint stays pending until acquire and submit complete"
        );

        driver.output_valid = true;
        assert!(driver.output_valid, "successful submit restores validity");

        let skip = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip.mode, PresentMode::SkipNoop));
        assert!(
            driver.output_valid,
            "SkipNoop preserves valid target pixels"
        );

        driver.strategy = PresentStrategy::BackbufferCopy;
        let skip_copy = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip_copy.mode, PresentMode::SkipCopy));
        assert!(
            !driver.output_valid,
            "SkipCopy stays pending until the copy is submitted"
        );
        driver.output_valid = true;
        assert!(driver.output_valid, "successful copy restores validity");
    }
}

#[cfg(test)]
mod record_store_tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::app::App;
    use crate::app::internals::RecordApp;
    use crate::host::clock::FixedClock;
    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::primitives::color::{Color, ColorU8};
    use crate::primitives::mesh::{Mesh, MeshVertex};
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::Frontend;
    use crate::shape::Shape;
    use crate::shape::polyline::PolylineColors;
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameProcessing;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;
    use crate::{Configure, Display, WindowToken};

    #[derive(Debug, PartialEq)]
    struct RecordPayloadSnapshot {
        mesh_vertices: Vec<MeshVertex>,
        mesh_indices: Vec<u32>,
        polyline_points: Vec<Vec2>,
        polyline_colors: Vec<ColorU8>,
        text: String,
    }

    #[derive(Debug, Default)]
    struct LifecycleApp {
        updates: Vec<WindowToken>,
        records: Vec<WindowToken>,
    }

    impl App for LifecycleApp {
        fn update(&mut self, win: WindowToken, _ui: &Ui) {
            self.updates.push(win);
        }

        fn record(&mut self, win: WindowToken, _ui: &mut Ui) {
            self.records.push(win);
        }
    }

    fn snapshot(driver: &WindowDriver) -> RecordPayloadSnapshot {
        let payloads = driver.ui.forest.record_store.payloads.borrow();
        RecordPayloadSnapshot {
            mesh_vertices: payloads.meshes.vertices.clone(),
            mesh_indices: payloads.meshes.indices.clone(),
            polyline_points: payloads.polyline_points.clone(),
            polyline_colors: payloads.polyline_colors.clone(),
            text: payloads.interned_text().bytes.to_owned(),
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
                    .diameter(92.0)
                    .show(ui);
            });
    }

    #[test]
    fn cpu_frame_forwards_token_through_app_lifecycle() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
        let token = WindowToken(17);
        let mut window = WindowDriver::builder(token, &shared)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .pixel_snap(false)
            .build();
        assert_eq!(window.strategy, PresentStrategy::DirectAdaptive);
        assert!(!window.pixel_snap);
        assert_eq!(window.clock.now(), Duration::ZERO);
        let mut app = LifecycleApp::default();

        let _ = window.cpu_frame(
            &mut frontend,
            Display::from_physical(UVec2::new(112, 112), 1.0),
            &mut app,
        );

        assert_eq!(app.updates, [token], "update runs once");
        assert_eq!(
            app.records,
            [token, token],
            "cold-start warmup and visible pass share the token",
        );
    }

    /// A record pass in one window must not replace the payloads retained by
    /// another window's animation-only frame.
    #[test]
    fn interleaved_window_paint_only_preserves_record_payloads() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
        let mut window_a = WindowDriver::builder(WindowToken(1), &shared)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let mut window_b = WindowDriver::builder(WindowToken(2), &shared)
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

        let mut app_a = RecordApp::new(|ui| {
            record_scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
        });
        let _ = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        window_a.output_valid = true;
        let retained = snapshot(&window_a);
        assert_eq!(retained.mesh_vertices.len(), 3);
        assert_eq!(retained.polyline_points.len(), 4);
        assert_eq!(retained.text, "retained A");

        let mut app_b = RecordApp::new(|ui| {
            record_scene(
                ui,
                &mesh_b,
                &points_b,
                &colors_b,
                "window B has a much longer label",
                "window-b",
            );
        });
        let _ = window_b.cpu_frame(&mut frontend, display, &mut app_b);
        window_b.output_valid = true;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}
