//! `WindowDriver`: the target-agnostic state one host target owns around the
//! shared renderer: its [`FrameLoop`] and its [`RenderStream`], which keeps what its frames hold on
//! the device. The host owns the time and hands it in with each [`FrameSpec`]. CPU scratch
//! lives on the shared [`Frontend`], shared GPU resources on the `WgpuBackend`, so N
//! windows share one renderer without sharing frame-local geometry.

pub(crate) mod present_facts;
mod screenshot_lane;

use crate::common::tracy;
use crate::damage::{Damage, DamageBase};
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::display::system_scale::SystemScale;
use crate::gpu::frame::submission::{self, FrameOverlay, Route, Submission};
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutMode};
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::host::window_driver::present_facts::{
    Escalation, PaintPath, PresentFacts, promote_share,
};
use crate::host::window_driver::screenshot_lane::ScreenshotLane;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::input_delta::InputDelta;
use crate::renderer::frontend::{BuildPlan, FrameOutput, Frontend};
use crate::renderer::pane_captures::{PaneCaptures, PaneDamage, Settled};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::ui::frame_loop::{FrameLoop, SubmitParts};
use crate::ui::frame_report::FrameReport;
use crate::ui::resources::UiResources;
#[cfg(feature = "winit")]
use crate::window::vsync::Vsync;
use crate::window::waker::Waker;
use crate::window::window_commands::WindowCommands;
#[cfg(feature = "winit")]
use crate::window::window_frame_state::WindowFacts;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::time::Duration;

/// Per-window state driving the host's shared [`Frontend`] and [`WgpuBackend`]. A host reaches a
/// window through it, and its [`FrameLoop`] keeps its engines private. Cost: a dozen forwarding
/// methods. Refused: one type, as `FrameLoop` sits in `ui`, left of `host`, and needs nothing from
/// the device.
#[derive(Debug)]
pub(super) struct WindowDriver {
    /// The window's `Ui` and the engines its frames run on.
    frames: FrameLoop,
    /// What the window's frames keep on the device.
    stream: RenderStream,
    /// What the next frame's damage diffs against: the last frame, once it completed its
    /// presentation. Discarded while a paint/copy is pending or after target invalidation.
    damage_base: DamageBase,
    /// Whether the target holds the damage outline the last painted frame drew onto it, which a
    /// frame that keeps the target's pixels would keep too.
    target_outlined: bool,
    strategy: PresentStrategy,
    /// The target the last [`Self::note_target`] saw.
    target: Option<TargetKey>,
    /// The captures this window's panes hold, which decide what a frame captures again.
    pane_captures: PaneCaptures,
    screenshots: ScreenshotLane,
    /// What the next frame paints in place of its own damage, for a bench that measures a decision.
    #[cfg(feature = "internals")]
    forced: internals::ForcedFrame,
}

/// What a window's driver opens as: the window's token, the render owner its frames draw as, and
/// how it presents.
#[derive(Clone, Copy, Debug)]
pub(super) struct DriverSpec {
    pub(super) token: WindowToken,
    pub(super) owner: RenderOwnerId,
    pub(super) strategy: PresentStrategy,
}

/// Identity of the surface or texture a window renders into: the single gate on retained target
/// state. A swapchain that rebuilds at the same key says so through
/// [`WindowDriver::invalidate_target_contents`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TargetKey {
    pub(super) physical: UVec2,
    pub(super) format: TargetFormat,
    /// Whether pixels can be copied out of the target, as a pane's capture is.
    pub(super) gives_copy: bool,
}

impl TargetKey {
    /// The key of the texture `target` names.
    pub(super) fn of(target: RenderTarget<'_>) -> Self {
        Self {
            physical: target.size(),
            format: target.format(),
            gives_copy: target.gives_copy(),
        }
    }
}

/// What a host knows of the frame it is about to run: the target, the platform's scale and
/// refresh rate, and the time.
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameSpec {
    pub(super) target: TargetKey,
    pub(super) system_scale: SystemScale,
    pub(super) refresh_millihertz: Option<u32>,
    pub(super) now: Duration,
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PresentStrategy {
    /// Prior contents can't be relied on (screenshots, visual harness): every frame
    /// renders into the backbuffer and copies out.
    BackbufferCopy,
    /// A swapchain, whose images hold nothing, and the host owns skip frames: each frame takes
    /// the cheaper of a direct whole repaint and a partial one through the backbuffer
    /// ([`PresentFacts::path`]).
    #[cfg_attr(
        not(any(feature = "winit", feature = "internals")),
        expect(
            dead_code,
            reason = "the winit host and the `internals` hosts that present as it build it"
        )
    )]
    DirectAdaptive,
    /// One texture that keeps its pixels between frames: a painted frame paints into it, a
    /// partial one loading what it holds, and a still frame presents nothing. A frame that takes
    /// a screenshot, or a partial one that cannot keep the target's pixels as they are
    /// (`WindowDriver::kept_target_path`), paints through the backbuffer.
    Retained,
}

/// What a frame's GPU half does, sealed in [`WindowDriver::build_paint`]. A frame that paints holds
/// the one frontend its draw list sits in, and `submit` consumes it, as `Widget::record` takes
/// `self`, so no other window can build until this one is submitted or dropped, and no stale path
/// survives. A frame that draws, dropped on a failed acquire or a lost device, leaves the base
/// discarded, and the next frame paints whole. Refused: the path, the scissors and the draw list in
/// three owners, held together by convention; typestate markers on `WindowDriver`
/// ([typestate](https://cliffle.com/blog/rust-typestate/)), as a driver lives in a `Vec` across
/// frames.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "built once a frame on the stack; boxing it would allocate per frame"
)]
pub(super) enum BuiltFrame<'f> {
    /// Nothing to draw, and the target holds the last frame.
    Still,
    /// Nothing to draw, and the target takes the backbuffer's copy of the last frame.
    CopyBackbuffer,
    Paint(PaintJob<'f>),
}

/// A built frame that paints: its path and scissors, and its draw list.
#[derive(Debug)]
pub(super) struct PaintJob<'f> {
    fitted: Fitted,
    frontend: &'f mut Frontend,
}

/// A frame's path, and the scissors fitted to its plan.
#[derive(Debug)]
struct Fitted {
    path: PaintPath,
    scissors: RepaintScissors,
}

impl Fitted {
    const fn whole(path: PaintPath) -> Self {
        Self {
            path,
            scissors: RepaintScissors::Full,
        }
    }
}

/// What a frame that paints builds with, past its record: the frontend every window composes
/// through, and the backend's placements its shadow cutouts plan at cells of.
#[derive(Debug)]
pub(super) struct BuildCtx<'f, 'p> {
    pub(super) frontend: &'f mut Frontend,
    pub(super) placements: &'p mut CutoutPlacements,
}

/// The CPU half's result: the report, and what the GPU half does.
#[derive(Debug)]
pub(super) struct CpuFrame<'f> {
    pub(super) report: FrameReport,
    pub(super) built: BuiltFrame<'f>,
}

impl WindowDriver {
    /// A driver as `spec` says over the shared [`UiResources`], on a detached stream until the
    /// host [attaches](Self::attach) its device's. Its [`FrameLoop`] holds the window's place in
    /// the window directory.
    pub(super) fn new(spec: DriverSpec, resources: &UiResources) -> Self {
        let DriverSpec {
            token,
            owner,
            strategy,
        } = spec;
        Self {
            frames: FrameLoop::new(token, resources),
            stream: RenderStream::detached(owner, CutoutMode::Shade),
            damage_base: DamageBase::Discarded,
            target_outlined: false,
            strategy,
            target: None,
            pane_captures: PaneCaptures::default(),
            screenshots: ScreenshotLane::default(),
            #[cfg(feature = "internals")]
            forced: internals::ForcedFrame::default(),
        }
    }

    /// The window's `Ui`.
    pub(super) const fn ui(&self) -> &Ui {
        &self.frames.ui
    }

    /// The window's `Ui`, mutably.
    pub(super) const fn ui_mut(&mut self) -> &mut Ui {
        &mut self.frames.ui
    }

    /// The window the app records for.
    #[cfg(feature = "winit")]
    pub(super) const fn token(&self) -> WindowToken {
        self.frames.token()
    }

    /// See [`FrameLoop::is_first_frame`].
    pub(super) const fn is_first_frame(&self) -> bool {
        self.frames.is_first_frame()
    }

    /// See [`FrameLoop::on_input`].
    pub(super) fn on_input(&mut self, event: InputEvent<'_>, now: Duration) -> InputDelta {
        self.frames.on_input(event, now)
    }

    /// See [`FrameLoop::set_waker`].
    pub(super) fn set_waker(&mut self, waker: Waker) {
        self.frames.set_waker(waker);
    }

    /// See [`FrameLoop::set_window_facts`].
    #[cfg(feature = "winit")]
    pub(super) fn set_window_facts(&mut self, facts: WindowFacts) {
        self.frames.set_window_facts(facts);
    }

    /// See [`FrameLoop::seed_vsync`].
    #[cfg(feature = "winit")]
    pub(super) const fn seed_vsync(&mut self, vsync: Vsync) {
        self.frames.seed_vsync(vsync);
    }

    /// See [`FrameLoop::laid_out_scale`].
    #[cfg(feature = "winit")]
    pub(super) fn laid_out_scale(&self) -> Option<f32> {
        self.frames.laid_out_scale()
    }

    /// See [`FrameLoop::drain_window_output`].
    pub(super) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        self.frames.drain_window_output(commands)
    }

    /// See [`FrameLoop::set_prev_stamp`].
    pub(super) const fn set_prev_stamp(&mut self, stamp: Option<FrameStamp>) {
        self.frames.set_prev_stamp(stamp);
    }

    /// See [`FrameLoop::set_display`].
    pub(super) const fn set_display(&mut self, display: Display) {
        self.frames.set_display(display);
    }

    /// The owner the window's frames render as.
    pub(super) const fn render_owner(&self) -> RenderOwnerId {
        self.stream.owner()
    }

    /// Draw through `stream`, the window's on a device the host attached, at startup and after a
    /// loss alike. What the window kept on an earlier device is gone with it: the pane ledger
    /// starts over, the target's contents read invalid, and each screenshot on its way back from
    /// that device goes back to the requests, ahead of the asks queued since. On a new driver
    /// these hold nothing.
    pub(super) fn attach(&mut self, stream: RenderStream) {
        debug_assert_eq!(
            stream.owner(),
            self.stream.owner(),
            "a window keeps its owner"
        );
        self.stream = stream;
        self.pane_captures = PaneCaptures::default();
        self.invalidate_target_contents();
        self.screenshots.requeue_pending();
    }

    /// The stamp of the frame `spec` describes, as [`Self::record`] runs it.
    ///
    /// The one place the user scale and the host's pixel snapping reach a frame: both are read
    /// here, as either can move mid-frame, so a copy held by the host would paint a stale frame.
    pub(super) fn stamp(&self, spec: &FrameSpec) -> FrameStamp {
        let display = Display {
            physical: spec.target.physical,
            system_scale: spec.system_scale,
            user_scale: self.frames.ui.user_scale(),
            pixel_snap: self.frames.pixel_snap(),
            refresh_millihertz: spec.refresh_millihertz,
        };
        FrameStamp::new(display, spec.now)
    }

    /// Whether a screenshot is on its way back from the device, which a later frame lands.
    pub(super) fn has_pending_screenshots(&self) -> bool {
        self.screenshots.has_pending()
    }

    /// Whether the device's copy of every screenshot owed has landed, or failed, so the next frame
    /// hands the oldest over.
    pub(super) fn screenshots_landed(&self) -> bool {
        self.screenshots.all_landed()
    }

    /// Declare the target of the frame `spec` describes. A change invalidates retained pixels and
    /// the damage baseline.
    fn note_target(&mut self, spec: &FrameSpec) {
        let key = spec.target;
        if self.target != Some(key) {
            self.target = Some(key);
            self.invalidate_target_contents();
        }
    }

    /// Declare the target's contents gone. Every swapchain rebuild owes this:
    /// [`Self::note_target`] makes it when the key moves, and the winit host when its
    /// swapchain rebuilds at the same key (new images: a lost, stale or repaced one).
    pub(super) const fn invalidate_target_contents(&mut self) {
        self.damage_base = DamageBase::Discarded;
        self.stream.invalidate();
    }

    /// The stage every host runs: the app's lifecycle under the display minted from `spec`, record
    /// through damage. The host picks what follows: [`Self::build_paint`] for a frame that paints,
    /// else [`Self::finish_unpainted`], as
    /// [branches gather at the top](https://matklad.github.io/2023/11/15/push-ifs-up-and-fors-down.html).
    /// Refused: a depth parameter, which rode the frontend and the cutout placements through calls
    /// that read neither; a `Frame<'a>` context over the whole stack, which hides what each stage
    /// reads; a `HostCore` method per step, as a headless host holds its device beside its
    /// `HeadlessWindow`.
    pub(super) fn record<T: App>(&mut self, spec: FrameSpec, app: &mut T) -> FrameReport {
        tracy::zone!();
        self.note_target(&spec);
        let stamp = self.stamp(&spec);
        self.frames.frame(stamp, self.damage_base, app)
    }

    /// Close a frame that paints nothing, as a host with no device runs. It keeps the damage
    /// baseline, as no target exists to lose it, and drops the screenshots asked in it, as no
    /// device takes them: no image lands for them, as for an empty ask.
    pub(super) fn finish_unpainted(&mut self) {
        self.frames
            .take_screenshot_requests(&mut self.screenshots.requests);
        self.screenshots.requests.clear();
        self.damage_base = DamageBase::Prev;
    }

    /// Build the draw list of `plan` under the panes it captures, as a painted frame builds it,
    /// for no target: a partial frame whose captures overflow its scissors builds whole.
    pub(super) fn compose(&mut self, frontend: &mut Frontend, plan: RenderPlan) {
        let mut plan = plan;
        if self.decide_panes(plan) == Settled::Overflow {
            plan = plan.to_full();
            self.decide_panes(plan);
        }
        self.build(frontend, plan);
        self.pane_captures.settle(&frontend.buffer);
    }

    /// Drop from the pane ledger the results the shared backdrop atlas evicted from this window's
    /// panes, so its next frame captures them again.
    pub(super) fn forget_evicted_panes(&mut self, backend: &mut WgpuBackend) {
        let ledger = &mut self.pane_captures;
        backend.drain_evicted_panes(self.stream.owner(), |key| ledger.forget(key));
    }

    /// Hand the next frame the oldest screenshot the device has finished, or none: the image is
    /// an edge of exactly that frame.
    pub(super) fn deliver_screenshot(&mut self, backend: &WgpuBackend) {
        let landed = self.screenshots.take_landed(backend);
        self.frames.set_screenshot_taken(landed);
    }

    /// Decide how the frame `report` describes reaches its target, and build its draw list. Seals
    /// the [`BuiltFrame`]: a promoted or resynced Partial builds a Full list.
    pub(super) fn build_paint<'f>(
        &mut self,
        ctx: BuildCtx<'f, '_>,
        mut report: FrameReport,
    ) -> CpuFrame<'f> {
        tracy::zone!();
        #[cfg(feature = "internals")]
        self.apply_forced(&mut report);
        self.frames
            .take_screenshot_requests(&mut self.screenshots.requests);
        let capturing = self.screenshots.is_capturing();
        if capturing {
            report.plan = Some(report.plan.map_or(
                RenderPlan {
                    clear: self.frames.ui.theme().window_bg,
                    damage: Damage::Full,
                },
                RenderPlan::to_full,
            ));
        }
        let built = if let Some(plan) = report.plan {
            let facts = PresentFacts {
                strategy: self.strategy,
                capturing,
                backbuffer_fresh: self.stream.is_backbuffer_fresh(),
                promote_share: matches!(plan.damage, Damage::Partial(_)).then(|| {
                    let scene = self.frames.ui.frame_scene();
                    promote_share(
                        scene.visible_row_count(),
                        scene.pane_share(),
                        scene.stamp.display.physical,
                    )
                }),
            };
            BuiltFrame::Paint(self.build_painted(ctx, facts.path(plan), facts))
        } else if self.strategy == PresentStrategy::BackbufferCopy {
            BuiltFrame::CopyBackbuffer
        } else {
            debug_assert_eq!(
                self.damage_base,
                DamageBase::Prev,
                "a discarded base paints whole, so a frame with nothing to draw holds the last"
            );
            BuiltFrame::Still
        };
        if !matches!(built, BuiltFrame::Still) {
            self.damage_base = DamageBase::Discarded;
        }
        // In the report, as an offscreen caller has no other sign that the window owes a frame.
        if self.screenshots.owes_frame() {
            report.repaint_requested = true;
        }
        CpuFrame { report, built }
    }

    /// Build the frame `first` paints: as it says when it can, else whole.
    fn build_painted<'f>(
        &mut self,
        mut ctx: BuildCtx<'f, '_>,
        first: PaintPath,
        facts: PresentFacts,
    ) -> PaintJob<'f> {
        let mut fitted = if matches!(first.plan.damage, Damage::Full) {
            self.build_whole(&mut ctx, first.plan);
            Fitted::whole(first)
        } else {
            match self.try_partial(&mut ctx, first, facts) {
                Ok(fitted) => fitted,
                Err(escalation) => {
                    let whole = first.plan.to_full();
                    self.build_whole(&mut ctx, whole);
                    Fitted::whole(facts.whole_path(whole, escalation))
                }
            }
        };
        self.pane_captures.settle(&ctx.frontend.buffer);
        fitted.path = self.capture_path(fitted.path, &ctx.frontend.buffer);
        debug_assert!(
            matches!(
                (fitted.path.plan.damage, &fitted.scissors),
                (Damage::Full, RepaintScissors::Full)
                    | (Damage::Partial(_), RepaintScissors::Partial(_))
            ),
            "the scissors are fitted to the frame's plan"
        );
        PaintJob {
            fitted,
            frontend: ctx.frontend,
        }
    }

    /// Build the partial frame `path` paints, through the backbuffer if the target cannot keep its
    /// pixels. Each refusal is one way, so a frame runs this once and then [`Self::build_whole`],
    /// which cannot refuse.
    fn try_partial(
        &mut self,
        ctx: &mut BuildCtx<'_, '_>,
        path: PaintPath,
        facts: PresentFacts,
    ) -> Result<Fitted, Escalation> {
        let plan = path.plan;
        // More panes capture than the repaint holds scissors for.
        if self.decide_panes(plan) == Settled::Overflow {
            return Err(Escalation::ByStrategy);
        }
        let path = if self.loses_kept_pixels(path.route) {
            if !facts.backbuffer_fresh {
                return Err(Escalation::ThroughBackbuffer);
            }
            PaintPath::through_backbuffer(plan)
        } else {
            path
        };
        // The disjoint scissors outgrow their cap.
        let display = self.frames.ui.frame_scene().stamp.display;
        let Some(scissors) = RepaintScissors::new(
            plan.damage,
            &display,
            self.pane_captures.capture_footprints(),
        ) else {
            return Err(Escalation::ByStrategy);
        };
        self.build(ctx.frontend, plan);
        // The pixels this partial frame keeps show a cutout form the frame no longer picks. The
        // one refusal after a build, as the census reads what the frame draws.
        let census = self
            .stream
            .plan_cutouts(&ctx.frontend.buffer, &scissors, ctx.placements);
        if census != Census::Cur {
            return Err(Escalation::ByStrategy);
        }
        Ok(Fitted { path, scissors })
    }

    /// Build the whole frame `plan` paints. A whole frame captures every pane and keeps no pixel,
    /// so nothing can refuse it.
    fn build_whole(&mut self, ctx: &mut BuildCtx<'_, '_>, plan: RenderPlan) {
        debug_assert!(matches!(plan.damage, Damage::Full));
        let settled = self.decide_panes(plan);
        debug_assert_eq!(
            settled,
            Settled::Fits,
            "a whole frame has no scissors to fit"
        );
        self.build(ctx.frontend, plan);
        let census =
            self.stream
                .plan_cutouts(&ctx.frontend.buffer, &RepaintScissors::Full, ctx.placements);
        debug_assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
    }

    /// The ledger's tickets for a frame painted by `plan`.
    fn decide_panes(&mut self, plan: RenderPlan) -> Settled {
        let scene = self.frames.ui.frame_scene();
        self.pane_captures.decide(
            scene.panes(),
            scene.stamp.display,
            PaneDamage::of(plan, self.frames.dirty_panes()),
        )
    }

    /// Encode and compose the frame `plan` paints, under the ledger's tickets, and list every
    /// `GpuView` the window records as the backend's retention roster: recorded, not painted, so
    /// a culled unchanged view keeps its texture.
    fn build(&self, frontend: &mut Frontend, plan: RenderPlan) {
        frontend.build(
            self.frames.ui.frame_scene(),
            BuildPlan {
                render: plan,
                panes: self.pane_captures.tickets(),
            },
        );
        self.frames
            .gpu_views()
            .collect_live_targets(&mut frontend.buffer.live_targets);
    }

    /// Whether a partial frame on `route` cannot paint in the target it keeps its pixels in: only a
    /// retained target paints a partial frame direct, and it cannot when it holds the last frame's
    /// damage outline, or a pane may capture from a target that gives no copy. Decided before the
    /// frame builds, from the tickets, as the partial draw list culls what a whole frame would
    /// paint.
    fn loses_kept_pixels(&self, route: Route) -> bool {
        let gives_copy = self.target.is_some_and(|target| target.gives_copy);
        route == Route::Direct
            && (self.target_outlined || (!gives_copy && self.pane_captures.may_capture()))
    }

    /// `path` for a frame whose panes capture as `buffer` says: a direct whole frame paints
    /// through the backbuffer when its captures cannot copy out of the target, which gives no
    /// copy, or would copy more than the surface holds, where its blurs read the backbuffer and
    /// the copy onto the target moves the surface once; the backbuffer may be new, as the frame
    /// is whole. A direct partial frame keeps its pixels in the target alone, so its captures copy
    /// out of it ([`Self::loses_kept_pixels`]).
    fn capture_path(&self, path: PaintPath, buffer: &RenderBuffer) -> PaintPath {
        let captured = buffer.captured_area();
        if path.route != Route::Direct || captured == 0 {
            return path;
        }
        let target = self
            .target
            .expect("a frame that captures has noted its target");
        if matches!(path.plan.damage, Damage::Partial(_)) {
            debug_assert!(
                target.gives_copy,
                "a partial frame's captures copy out of it"
            );
            return path;
        }
        let surface = u64::from(target.physical.x) * u64::from(target.physical.y);
        if !target.gives_copy || captured >= surface {
            PaintPath::through_backbuffer(path.plan)
        } else {
            path
        }
    }

    /// Give back what `backend` keeps for this window, as it closes on that device.
    pub(super) fn retire(&mut self, backend: &mut WgpuBackend) {
        backend.retire_stream(&mut self.stream);
    }

    /// The GPU half: draw `built` into `target`. Whatever `built` holds, the target then holds this
    /// frame, so the next frame's damage diffs against it.
    pub(super) fn submit(
        &mut self,
        backend: &mut WgpuBackend,
        target: RenderTarget<'_>,
        built: BuiltFrame<'_>,
    ) {
        tracy::zone!();
        let size = target.size();
        let display_physical = self.frames.ui.display().physical;
        debug_assert!(
            size == display_physical,
            "submit: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `record` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `record`.",
            size.x,
            size.y,
            display_physical.x,
            display_physical.y,
        );
        debug_assert!(
            self.target
                .is_some_and(|key| key.physical == size && key.format == target.format()),
            "submit: target ({}x{}, {:?}) differs from the one \
             `note_target` declared ({:?}), so the retained backbuffer / damage \
             baseline were never invalidated for it",
            size.x,
            size.y,
            target.format(),
            self.target,
        );
        match built {
            BuiltFrame::Still => {}
            BuiltFrame::CopyBackbuffer => {
                // Nothing to draw implies the previous frame painted at this size and format, so
                // the backbuffer exists and matches.
                backend.copy_backbuffer_to_surface(&self.stream, target);
                self.target_outlined = false;
            }
            BuiltFrame::Paint(job) => self.submit_paint(backend, target, job),
        }
        self.damage_base = DamageBase::Prev;
    }

    /// Paint `job` into `target`, and take each screenshot the frame owes from it.
    fn submit_paint(
        &mut self,
        backend: &mut WgpuBackend,
        target: RenderTarget<'_>,
        job: PaintJob<'_>,
    ) {
        let PaintJob {
            fitted:
                Fitted {
                    path: PaintPath { route, plan },
                    scissors,
                },
            frontend,
        } = job;
        debug_assert!(
            route != Route::Direct || frontend.buffer.captured_area() == 0 || target.gives_copy(),
            "a direct frame's captures copy out of the target"
        );
        debug_assert!(
            self.strategy == PresentStrategy::Retained
                || route != Route::Direct
                || matches!(plan.damage, Damage::Full),
            "a direct partial frame loads the target, which only a retained one keeps: a \
             swapchain image holds nothing at acquire"
        );
        let FrameOutput { buffer, gradients } = frontend.output();
        let debug_overlay = self.frames.ui.debug_overlay();
        self.target_outlined = debug_overlay.damage_rect;
        let SubmitParts {
            stats,
            store,
            gpu_views,
        } = self.frames.submit_parts();
        backend.submit(
            &mut self.stream,
            target,
            Submission {
                stats,
                store,
                buffer,
                gradients,
                gpu_views,
                clear: submission::frame_clear(plan.clear, buffer),
                scissors: &scissors,
                route,
                overlay: FrameOverlay {
                    dim_undamaged: debug_overlay.dim_undamaged,
                    damage: debug_overlay.damage_rect.then_some(plan.damage),
                },
            },
        );
        debug_assert!(
            route == Route::ViaBackbuffer || !self.screenshots.is_capturing(),
            "a frame with a screenshot request paints through the backbuffer"
        );
        let display = self.frames.ui.display();
        self.screenshots.capture_all(backend, &self.stream, display);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::damage::Damage;
    #[cfg(test)]
    use crate::damage::DamageBase;
    #[cfg(test)]
    use crate::display::system_scale::SystemScale;
    use crate::host::window_driver::WindowDriver;
    #[cfg(test)]
    use crate::host::window_driver::present_facts::PaintPath;
    #[cfg(test)]
    use crate::host::window_driver::{BuiltFrame, FrameSpec, TargetKey};
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::frame_loop::FrameLoop;
    use crate::ui::frame_report::FrameReport;
    #[cfg(test)]
    use std::time::Duration;

    impl WindowDriver {
        /// The window's `Ui` and the engines its frames run on, for a harness's reads.
        pub(crate) const fn frames(&self) -> &FrameLoop {
            &self.frames
        }

        pub(crate) const fn frames_mut(&mut self) -> &mut FrameLoop {
            &mut self.frames
        }
    }

    #[cfg(test)]
    impl BuiltFrame<'_> {
        /// The path of a frame that paints, for a test that reads it.
        pub(crate) const fn path(&self) -> Option<PaintPath> {
            match self {
                Self::Still | Self::CopyBackbuffer => None,
                Self::Paint(job) => Some(job.fitted.path),
            }
        }
    }

    #[cfg(test)]
    impl FrameSpec {
        /// A frame laid out for `target`, at scale 1 and time zero, with no refresh rate. Test
        /// values: a host states each field from what it knows.
        pub(crate) const fn at_rest(target: TargetKey) -> Self {
            Self {
                target,
                system_scale: SystemScale::ONE,
                refresh_millihertz: None,
                now: Duration::ZERO,
            }
        }
    }

    /// What the next frame paints in place of its own damage.
    #[derive(Debug, Default)]
    pub(crate) struct ForcedFrame {
        damage: Option<Damage>,
        /// What the frame's content changed, which leaves each pane whose footprint it touches
        /// stale. Kept apart from `damage`, whose rects may have merged past a pane no change
        /// reached.
        changed: Vec<Rect>,
    }

    impl WindowDriver {
        /// Have the next frame that paints paint `damage` over a change of `changed`, past the
        /// damage engine's answer and the present path's promotion, so a bench can time a decision
        /// production would not make, on a scene that holds still.
        pub(crate) fn force_next_frame(&mut self, damage: Damage, changed: &[Rect]) {
            self.forced.damage = Some(damage);
            self.forced.changed.clear();
            self.forced.changed.extend_from_slice(changed);
        }

        /// Put the forced decision in `report`'s plan, and mark the panes its change left stale.
        pub(super) fn apply_forced(&mut self, report: &mut FrameReport) {
            let Some(damage) = self.forced.damage.take() else {
                return;
            };
            report.plan = Some(RenderPlan {
                clear: self.frames.ui.theme().window_bg,
                damage,
            });
            // A whole frame captures every pane and reports none stale.
            if matches!(damage, Damage::Full) {
                return;
            }
            self.frames.force_stale_panes(&self.forced.changed);
        }

        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.stream.disable_cutout_tables();
        }

        /// Whether the target holds this driver's last output; read by offscreen tests.
        #[cfg(test)]
        pub(crate) const fn output_valid(&self) -> bool {
            matches!(self.damage_base, DamageBase::Prev)
        }

        /// Whether a partial frame of this driver painted a damage canvas.
        #[cfg(test)]
        pub(crate) const fn holds_canvas(&self) -> bool {
            self.stream.holds_canvas()
        }
    }
}

#[cfg(test)]
mod tests;
