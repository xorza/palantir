//! `WindowDriver`: the target-agnostic state one host target owns around the
//! shared renderer: its [`Ui`], the persistent [`Backbuffer`], the shadow cutout
//! plan, and the frame clock. CPU scratch lives on the shared [`Frontend`], GPU
//! resources on the shared `WgpuBackend`, so N windows share one renderer without
//! sharing frame-local geometry.

use crate::app::App;
use crate::common::tracy;
use crate::damage::{Damage, FULL_REPAINT_THRESHOLD};
use crate::display::Display;
use crate::gpu::frame::submission::Submission;
use crate::gpu::frame::submission::SubmissionTargets;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutPlan};
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::build_repaint_scissors;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::host::clock::{Clock, RealtimeClock};
use crate::renderer::frontend::Frontend;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::FrameReport;
use crate::ui::frame_stamp::FrameInput;
use crate::ui::frame_stamp::FrameStamp;
use crate::ui::resources::UiResources;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::time::Duration;

/// Per-window state driving the host's shared [`Frontend`] and [`WgpuBackend`].
#[derive(Debug)]
pub(super) struct WindowDriver {
    pub(super) token: WindowToken,
    pub(super) ui: Ui,
    /// The layout / cascade / damage engines, held here out of authoring code's reach.
    engines: FrameEngines,
    pub(super) render_owner: RenderOwnerId,
    /// Off-screen target holding last frame's pixels for `LoadOp::Load` partial damage.
    /// Created lazily; recreated on resize or format change.
    backbuffer: Option<Backbuffer>,
    /// `true` when [`Self::backbuffer`] mirrors the target. A direct full frame leaves
    /// it stale; the next partial resyncs it with one full repaint.
    backbuffer_fresh: bool,
    /// Whether the last frame completed its presentation. Invalid while a paint/copy
    /// is pending or after target invalidation, so the next frame drops its damage
    /// baseline.
    output_valid: bool,
    /// Rounded-clip stencil, allocated lazily; separate so direct present needs no
    /// backbuffer.
    stencil: Option<Stencil>,
    strategy: PresentStrategy,
    /// Per-frame time source ([`RealtimeClock`] on screen,
    /// [`FixedClock`](crate::host::clock::FixedClock) offscreen).
    pub(super) clock: Box<dyn Clock>,
    pixel_snap: bool,
    /// The target the last [`Self::note_target`] saw.
    target: Option<TargetKey>,
    cutouts: CutoutPlan,
}

/// Identity of the surface or texture a window renders into. The single gate on
/// retained target state; on a swapchain host it also decides surface
/// reconfiguration, so a swapchain setting that becomes mutable must be added here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TargetKey {
    pub(super) physical: UVec2,
    pub(super) format: TargetFormat,
    pub(super) vsync: Option<Vsync>,
}

impl TargetKey {
    pub(super) fn of(target: RenderTarget<'_>) -> Self {
        Self {
            physical: target.size(),
            format: target.format(),
            vsync: None,
        }
    }

    /// Whether this key describes a target with the given texture facts. [`PartialEq`]
    /// compares configuration and would never hold against a plain texture, which has
    /// no pacing.
    pub(super) fn describes(&self, physical: UVec2, format: TargetFormat) -> bool {
        self.physical == physical && self.format == format
    }
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PresentStrategy {
    /// Prior contents can't be relied on (screenshots, visual harness): every frame
    /// renders into the backbuffer and copies out.
    BackbufferCopy,
    /// Direct-present swapchain; the host owns skip frames. Full frames repaint into
    /// the target, small partials go through the backbuffer, near-full partials are
    /// promoted to a direct full repaint. A surface that can't be copied into gets the
    /// backbuffer drawn onto it ([`Backbuffer::draw_onto`](crate::gpu::surface::backbuffer::Backbuffer::draw_onto)).
    DirectAdaptive,
}

/// How a frame reaches the target, sealed in `cpu_frame` so the GPU half submits
/// the plan the draw list was built for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PresentPath {
    SkipCopy,
    SkipNoop,
    Direct(RenderPlan),
    ViaBackbuffer(RenderPlan),
}

impl PresentPath {
    /// Whether a frame in this mode renders through the backbuffer (`SkipCopy` only
    /// reads it).
    const fn renders_via_backbuffer(self) -> bool {
        matches!(self, Self::ViaBackbuffer(_))
    }
}

/// Coverage above which [`PresentStrategy::DirectAdaptive`] promotes a `Partial`
/// to a direct full repaint. Below [`FULL_REPAINT_THRESHOLD`] (asserted), so a
/// promotable partial isn't already `Full`. The backbuffer path pays a whole-surface
/// copy per frame; measured crossover is near 0.40 (`frame` bench, Radeon 680M:
/// `scrolling` 7.8 ms via backbuffer vs 6.8 ms direct; a tiny partial 3.3 ms via
/// backbuffer). Area only proxies cost, so the line sits under the expensive band.
const DIRECT_PROMOTE_COVERAGE: f32 = 0.4;

// Retuning either threshold past the other would silently kill promotion.
const _: () = assert!(DIRECT_PROMOTE_COVERAGE < FULL_REPAINT_THRESHOLD);

fn present_path(
    plan: Option<RenderPlan>,
    strategy: PresentStrategy,
    backbuffer_fresh: bool,
) -> PresentPath {
    match strategy {
        PresentStrategy::DirectAdaptive => match plan {
            None => PresentPath::SkipNoop,
            Some(p) => match p.damage {
                Damage::Full => PresentPath::Direct(p),
                Damage::Partial(damage) => {
                    if damage.coverage > DIRECT_PROMOTE_COVERAGE {
                        PresentPath::Direct(p.to_full())
                    } else if backbuffer_fresh {
                        PresentPath::ViaBackbuffer(p)
                    } else {
                        PresentPath::ViaBackbuffer(p.to_full())
                    }
                }
            },
        },
        PresentStrategy::BackbufferCopy => match plan {
            None => PresentPath::SkipCopy,
            Some(p) => PresentPath::ViaBackbuffer(p),
        },
    }
}

/// The CPU half's result: the report plus the [`PresentPath`] sealed at
/// draw-list-build time.
#[derive(Debug)]
pub(super) struct CpuFrame {
    pub(super) report: FrameReport,
    pub(super) mode: PresentPath,
}

#[derive(Debug)]
pub(super) struct WindowDriverBuilder<'a> {
    token: WindowToken,
    resources: &'a UiResources,
    strategy: PresentStrategy,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
    bake_cutouts: bool,
}

impl WindowDriverBuilder<'_> {
    pub(super) const fn strategy(mut self, strategy: PresentStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Takes the box: callers already boxed one, and `impl Clock` would box twice.
    pub(super) fn clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Whether to bake shadow cutout tables ([`WgpuBackend::bakes_cutouts`]); off
    /// shades every corner, which is right on any device.
    pub(super) const fn bake_cutouts(mut self, bake: bool) -> Self {
        self.bake_cutouts = bake;
        self
    }

    /// Registered here so a builder dropped unbuilt leaves no live token.
    pub(super) fn build(self) -> WindowDriver {
        self.resources.windows().add(self.token);
        WindowDriver {
            token: self.token,
            engines: FrameEngines::new(self.resources),
            ui: Ui::new(self.resources.clone()),
            render_owner: RenderOwnerId::reserve(),
            backbuffer: None,
            backbuffer_fresh: false,
            output_valid: false,
            stencil: None,
            strategy: self.strategy,
            clock: self.clock,
            pixel_snap: self.pixel_snap,
            target: None,
            cutouts: CutoutPlan::new(self.bake_cutouts),
        }
    }
}

/// Retires this driver's token from the
/// [`WindowDirectory`](crate::window::window_directory::WindowDirectory) on drop,
/// so no close path can leave `Ui::is_window_open` true.
impl Drop for WindowDriver {
    fn drop(&mut self) {
        self.ui.window_directory().remove(self.token);
    }
}

impl WindowDriver {
    /// This driver's clock; frames and input events must share it.
    pub(super) fn now(&self) -> Duration {
        self.clock.now()
    }

    /// Start building a driver for `token` from the shared [`UiResources`]. Defaults:
    /// direct adaptive presentation, realtime clock. `pixel_snap` is a parameter
    /// because every driver a host mints carries the host's.
    pub(super) fn builder(
        token: WindowToken,
        resources: &UiResources,
        pixel_snap: bool,
    ) -> WindowDriverBuilder<'_> {
        WindowDriverBuilder {
            token,
            resources,
            strategy: PresentStrategy::DirectAdaptive,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap,
            bake_cutouts: false,
        }
    }

    /// This driver's [`Display`] for a frame. The one place `pixel_snap` and the user
    /// scale reach a frame: the user scale is written mid-frame, so a host-cached copy
    /// would paint a stale frame after it moves.
    pub(super) fn display(
        &self,
        physical: UVec2,
        system_scale: f32,
        refresh_millihertz: Option<u32>,
    ) -> Display {
        Display {
            physical,
            system_scale,
            user_scale: self.ui.user_scale(),
            pixel_snap: self.pixel_snap,
            refresh_millihertz,
        }
    }

    /// Declare the target the next [`Self::cpu_frame`] renders into, returning
    /// whether it changed. A change invalidates retained pixels and the damage
    /// baseline; a swapchain adapter also reconfigures on `true`.
    pub(super) fn note_target(&mut self, key: TargetKey) -> bool {
        if self.target == Some(key) {
            return false;
        }
        self.target = Some(key);
        self.invalidate_target_contents();
        true
    }

    /// Declare the target's contents gone. Every `surface.configure` owes this:
    /// [`Self::note_target`] makes it when the key moves, and the winit host where it
    /// reconfigures a lost or suboptimal swapchain (new images, same key).
    pub(super) const fn invalidate_target_contents(&mut self) {
        self.output_valid = false;
        self.backbuffer_fresh = false;
    }

    /// The shared CPU half: app lifecycle, record through damage, then the draw-list
    /// build if the frame paints. Seals the [`PresentPath`] (a promoted or resynced
    /// Partial builds a Full list).
    pub(super) fn cpu_frame<T: App>(
        &mut self,
        frontend: &mut Frontend,
        display: Display,
        app: &mut T,
    ) -> CpuFrame {
        tracy::zone!();
        let report = self.ui.frame(
            &mut self.engines,
            FrameInput::new(
                FrameStamp::new(display, self.clock.now()),
                self.output_valid,
            ),
            self.token,
            app,
        );
        self.finish_cpu_frame(frontend, report)
    }

    fn finish_cpu_frame(&mut self, frontend: &mut Frontend, report: FrameReport) -> CpuFrame {
        let mut mode = present_path(report.plan, self.strategy, self.backbuffer_fresh);
        if !matches!(mode, PresentPath::SkipNoop) {
            self.output_valid = false;
        }
        if let PresentPath::Direct(plan) | PresentPath::ViaBackbuffer(plan) = mode {
            frontend.build(self.ui.frame_scene(), plan);
            if self.plan_cutouts(&frontend.buffer, plan) == Census::Stale {
                // The pixels this partial frame keeps show a cutout form the frame no longer picks.
                let full = plan.to_full();
                mode = present_path(Some(full), self.strategy, self.backbuffer_fresh);
                frontend.build(self.ui.frame_scene(), full);
                let census = self.plan_cutouts(&frontend.buffer, full);
                debug_assert_eq!(census, Census::Current, "a full repaint keeps no pixel");
            }
        }
        CpuFrame { report, mode }
    }

    fn plan_cutouts(&mut self, buffer: &RenderBuffer, plan: RenderPlan) -> Census {
        let scissors = build_repaint_scissors(plan.damage, buffer);
        self.cutouts
            .build(&buffer.quads, &scissors, buffer.display.physical)
    }

    /// [`Ui::drain_window_output`] bound to this driver's token; the offscreen
    /// adapter drains into a dropped scratch buffer.
    pub(super) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        self.ui.drain_window_output(self.token, commands)
    }

    /// [`Self::drain_window_output`] for a host with no window lifecycle. Levels
    /// (cursor, vsync) stay with the recorder and are inert here; commands (open,
    /// close) mean nothing unserviced, so they are a caller error.
    ///
    /// # Panics
    ///
    /// Panics if this frame recorded any window open or close request.
    pub(super) fn deny_window_commands(&mut self) {
        // Empty by contract, so the drain's `append`s move nothing and nothing allocates.
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

    /// GPU submit against a caller-supplied texture, dispatching on the sealed
    /// [`PresentPath`]. [`PresentPath::SkipCopy`] copies the backbuffer onto `target`.
    pub(super) fn render_to_texture(
        &mut self,
        buffer: &RenderBuffer,
        backend: &mut WgpuBackend,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        tracy::zone!();
        let size = target.size();
        let display_phys = self.ui.display().physical;
        debug_assert!(
            size == display_phys,
            "render_to_texture: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `cpu_frame` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `cpu_frame`.",
            size.x,
            size.y,
            display_phys.x,
            display_phys.y,
        );
        debug_assert!(
            self.target
                .is_some_and(|key| key.describes(size, target.format())),
            "render_to_texture: target ({}x{}, {:?}) differs from the one \
             `note_target` declared ({:?}), so the retained backbuffer / damage \
             baseline were never invalidated for it",
            size.x,
            size.y,
            target.format(),
            self.target,
        );
        let debug_overlay = self.ui.debug_overlay();
        // Shared by both paint paths. Gated: on a skip frame `buffer.rounded_clips` is
        // stale.
        let stencil = match mode {
            PresentPath::Direct(_) | PresentPath::ViaBackbuffer(_)
                if !buffer.rounded_clips.is_empty() =>
            {
                Some(Stencil::ensure(&mut self.stencil, backend.device(), size))
            }
            _ => None,
        };
        match mode {
            PresentPath::SkipNoop => self.output_valid = true,
            PresentPath::SkipCopy => {
                // A `Skip` implies the previous frame painted at this size and format, so the
                // backbuffer exists and matches.
                let bb = self
                    .backbuffer
                    .as_ref()
                    .expect("SkipCopy implies a prior submitted paint frame");
                backend.copy_backbuffer_to_surface(bb, target);
                self.output_valid = true;
            }
            // A direct repaint leaves the mirror stale; one through the backbuffer matches
            // the target.
            PresentPath::Direct(plan) | PresentPath::ViaBackbuffer(plan) => {
                let backbuffer = if mode.renders_via_backbuffer() {
                    let ensured = Backbuffer::ensure(
                        &mut self.backbuffer,
                        backend.device(),
                        backend.texture_binding(),
                        size,
                        target.format(),
                    );
                    // A Partial arrives un-escalated only when `backbuffer_fresh`, so a recreate
                    // means that broke; the list was already Partial-culled, so escalating can't fix it.
                    debug_assert!(
                        !ensured.recreated || matches!(plan.damage, Damage::Full),
                        "backbuffer (re)created under a Partial plan whose draw \
                         list was culled for Partial"
                    );
                    Some(ensured.backbuffer)
                } else {
                    None
                };
                self.backbuffer_fresh = backbuffer.is_some();
                let store = self.ui.record_store();
                backend.submit(Submission {
                    owner: self.render_owner,
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer,
                        stencil,
                    },
                    store,
                    buffer,
                    plan,
                    cutouts: &self.cutouts,
                    debug_overlay,
                });
                self.output_valid = true;
            }
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutPlan;
    use crate::host::window_driver::WindowDriver;

    impl WindowDriver {
        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.cutouts = CutoutPlan::new(false);
        }

        /// Whether the target holds this driver's last output; read by offscreen tests.
        #[cfg(test)]
        pub(crate) const fn output_valid(&self) -> bool {
            self.output_valid
        }
    }
}

#[cfg(test)]
mod tests;
