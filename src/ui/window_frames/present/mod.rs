//! How a window's built frame reaches its target: the host's facts about the target, the present
//! decision, and the GPU half. [`Frontend`] holds the CPU scratch and `WgpuBackend` the GPU
//! resources every window shares, so N windows share one renderer without sharing frame-local
//! geometry.

pub(crate) mod present_facts;
pub(crate) mod screenshot_lane;

use crate::common::tracy;
use crate::damage::{Damage, DamageBase};
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::display::system_scale::SystemScale;
use crate::gpu::frame::submission::{self, FrameOverlay, Route, Submission};
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacing;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::Census;
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::texel::TargetFormat;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::renderer::backdrop_captures::{BackdropCaptures, BackdropDamage, Settled};
use crate::renderer::frontend::{BuildPlan, Frontend};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::renderer::row_table::RowTable;
use crate::ui::app::App;
use crate::ui::frame_report::FrameReport;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::present_facts::{
    Escalation, PaintPath, PresentFacts, promote_share,
};
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::num::NonZeroU32;
use std::time::Duration;

/// What a window opens as: its token, the render owner its frames draw as, and how it presents.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowSpec {
    pub(crate) token: WindowToken,
    pub(crate) owner: RenderOwnerId,
    pub(crate) strategy: PresentStrategy,
}

/// Identity of the surface or texture a window renders into: the single gate on retained target
/// state. A swapchain that rebuilds at the same key says so through
/// [`WindowFrames::invalidate_target_contents`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetKey {
    pub(crate) physical: UVec2,
    pub(crate) format: TargetFormat,
    /// Whether pixels can be copied out of the target, as a backdrop's capture is.
    pub(crate) gives_copy: bool,
}

impl TargetKey {
    /// The key of the texture `target` names.
    pub(crate) fn of(target: RenderTarget<'_>) -> Self {
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
pub(crate) struct FrameSpec {
    pub(crate) target: TargetKey,
    pub(crate) system_scale: SystemScale,
    pub(crate) refresh_millihertz: Option<NonZeroU32>,
    pub(crate) now: Duration,
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PresentStrategy {
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
    /// (`WindowFrames::loses_kept_pixels`), paints through the backbuffer.
    Retained,
}

/// What a frame's GPU half does, sealed in [`WindowFrames::build_paint`]. A frame that paints holds
/// the one frontend its draw list sits in, and `submit` consumes it, as `Widget::record` takes
/// `self`, so no other window can build until this one is submitted or dropped, and no stale path
/// survives. A frame that draws, dropped on a failed acquire or a lost device, leaves the base
/// discarded, and the next frame paints whole. Refused: the path, the scissors and the draw list in
/// three owners, held together by convention; typestate markers on `WindowFrames`
/// ([typestate](https://cliffle.com/blog/rust-typestate/)), as a window lives in a `Vec` across
/// frames.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "built once a frame on the stack; boxing it would allocate per frame"
)]
pub(crate) enum BuiltFrame<'f> {
    /// Nothing to draw, and the target holds the last frame.
    Still,
    /// Nothing to draw, and the target takes the backbuffer's copy of the last frame.
    CopyBackbuffer,
    Paint(PaintJob<'f>),
}

/// A built frame that paints: its path and scissors, and its draw list.
#[derive(Debug)]
pub(crate) struct PaintJob<'f> {
    fitted: Fitted,
    frontend: &'f Frontend,
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
/// through, the backend's pool with the placements its shadow cutouts plan in, whose row table its
/// gradients take rows in, and the display frame its submit will belong to, which a build that
/// paints moves the pool to first. Refused: the pool moved before every build, which a still frame
/// left a frame ahead of the last it drew, so a purge took what that frame drew; the pool left on
/// the last submit's frame, where an insert met that frame's dry latch and a release stamped a
/// frame early.
#[derive(Debug)]
pub(crate) struct BuildCtx<'f, 'p> {
    pub(crate) frontend: &'f mut Frontend,
    pub(crate) placing: CutoutPlacing<'p>,
    pub(crate) frame: u64,
}

/// The CPU half's result: the report, and what the GPU half does.
#[derive(Debug)]
pub(crate) struct CpuFrame<'f> {
    pub(crate) report: FrameReport,
    pub(crate) built: BuiltFrame<'f>,
}

impl WindowFrames {
    /// The owner the window's frames render as.
    pub(crate) const fn render_owner(&self) -> RenderOwnerId {
        self.stream.owner()
    }

    /// Draw through `stream`, the window's on a device the host attached, at startup and after a
    /// loss alike. What the window kept on an earlier device is gone with it: the backdrop ledger
    /// starts over, the target's contents read invalid, and each screenshot on its way back from
    /// that device goes back to the requests, ahead of the asks queued since. On a new window
    /// these hold nothing.
    pub(crate) fn attach(&mut self, stream: RenderStream) {
        debug_assert_eq!(
            stream.owner(),
            self.stream.owner(),
            "a window keeps its owner"
        );
        self.stream = stream;
        self.backdrop_captures = BackdropCaptures::default();
        self.invalidate_target_contents();
        self.screenshots.requeue_pending();
    }

    /// The stamp of the frame `spec` describes, as [`Self::record`] runs it.
    ///
    /// The one place the user scale and the host's pixel snapping reach a frame: both are read
    /// here, as either can move mid-frame, so a copy held by the host would paint a stale frame.
    pub(crate) fn stamp(&self, spec: &FrameSpec) -> FrameStamp {
        let display = Display {
            physical: spec.target.physical,
            system_scale: spec.system_scale,
            user_scale: self.ui.user_scale(),
            pixel_snap: self.ui.resources.settings().pixel_snap(),
            refresh_millihertz: spec.refresh_millihertz,
        };
        FrameStamp::new(display, spec.now)
    }

    /// Whether a screenshot is on its way back from the device, which a later frame lands.
    pub(crate) fn has_pending_screenshots(&self) -> bool {
        self.screenshots.has_pending()
    }

    /// Whether the device's copy of every screenshot owed has landed, or failed, so the next frame
    /// hands the oldest over.
    pub(crate) fn screenshots_landed(&self) -> bool {
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
    pub(crate) const fn invalidate_target_contents(&mut self) {
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
    pub(crate) fn record<T: App>(&mut self, spec: FrameSpec, app: &mut T) -> FrameReport {
        tracy::zone!();
        self.note_target(&spec);
        let stamp = self.stamp(&spec);
        self.frame(stamp, app)
    }

    /// Close a frame that paints nothing, as a host with no device runs. It keeps the damage
    /// baseline, as no target exists to lose it, and drops the screenshots asked in it, as no
    /// device takes them: no image lands for them, as for an empty ask.
    pub(crate) fn finish_unpainted(&mut self) {
        self.screenshots
            .requests
            .append(&mut self.ui.host.window_requests.screenshots);
        self.screenshots.requests.clear();
        self.damage_base = DamageBase::Prev;
    }

    /// Build the draw list of `plan` under the backdrops it captures, its gradients in `rows`, as a
    /// painted frame builds it, for no target: a partial frame whose captures overflow its scissors
    /// builds whole.
    pub(crate) fn compose(
        &mut self,
        frontend: &mut Frontend,
        rows: &mut RowTable,
        plan: RenderPlan,
    ) {
        let mut plan = plan;
        if self.decide_backdrops(plan) == Settled::Overflow {
            plan = plan.to_full();
            self.decide_backdrops(plan);
        }
        self.build(frontend, rows, plan);
        self.backdrop_captures.settle(&frontend.buffer);
    }

    /// Drop from the backdrop ledger the results the shared backdrop atlas evicted from this
    /// window's backdrops, so its next frame captures them again.
    pub(crate) fn forget_evicted_backdrops(&mut self, backend: &mut WgpuBackend) {
        let ledger = &mut self.backdrop_captures;
        backend.drain_evicted_backdrops(self.stream.owner(), |key| ledger.forget(key));
    }

    /// Hand the next frame the oldest screenshot the device has finished, or none: the image is
    /// an edge of exactly that frame.
    pub(crate) fn deliver_screenshot(&mut self, backend: &WgpuBackend) {
        let landed = self.screenshots.take_landed(backend);
        if landed.is_some() {
            self.ui.frame_runtime.request_repaint();
        }
        self.ui.host.window_frame.screenshot = landed.map(Box::new);
    }

    /// Decide how the frame `report` describes reaches its target, and build its draw list. Seals
    /// the [`BuiltFrame`]: a promoted or resynced Partial builds a Full list.
    pub(crate) fn build_paint<'f>(
        &mut self,
        ctx: BuildCtx<'f, '_>,
        mut report: FrameReport,
    ) -> CpuFrame<'f> {
        tracy::zone!();
        #[cfg(feature = "internals")]
        self.apply_forced(&mut report);
        self.screenshots
            .requests
            .append(&mut self.ui.host.window_requests.screenshots);
        let capturing = self.screenshots.is_capturing();
        if capturing {
            report.plan = Some(report.plan.map_or(
                RenderPlan {
                    clear: self.ui.theme().window_bg,
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
                    let scene = self.ui.frame_scene();
                    promote_share(
                        scene.visible_row_count(),
                        scene.backdrop_share(),
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
        ctx.placing.pool.advance_to(ctx.frame);
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
        self.backdrop_captures.settle(&ctx.frontend.buffer);
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
        // More backdrops capture than the repaint holds scissors for.
        if self.decide_backdrops(plan) == Settled::Overflow {
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
        let display = self.ui.frame_scene().stamp.display;
        let Some(scissors) = RepaintScissors::new(
            plan.damage,
            &display,
            self.backdrop_captures.capture_footprints(),
        ) else {
            return Err(Escalation::ByStrategy);
        };
        self.build(ctx.frontend, ctx.placing.pool.rows_mut(), plan);
        // The pixels this partial frame keeps show a cutout form the frame no longer picks. The
        // one refusal after a build, as the census reads what the frame draws.
        let census = self
            .stream
            .plan_cutouts(&ctx.frontend.buffer, &scissors, &mut ctx.placing);
        if census != Census::Cur {
            return Err(Escalation::ByStrategy);
        }
        Ok(Fitted { path, scissors })
    }

    /// Build the whole frame `plan` paints. A whole frame captures every backdrop and keeps no
    /// pixel, so nothing can refuse it.
    fn build_whole(&mut self, ctx: &mut BuildCtx<'_, '_>, plan: RenderPlan) {
        debug_assert!(matches!(plan.damage, Damage::Full));
        let settled = self.decide_backdrops(plan);
        debug_assert_eq!(
            settled,
            Settled::Fits,
            "a whole frame has no scissors to fit"
        );
        self.build(ctx.frontend, ctx.placing.pool.rows_mut(), plan);
        let census = self.stream.plan_cutouts(
            &ctx.frontend.buffer,
            &RepaintScissors::Full,
            &mut ctx.placing,
        );
        debug_assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
    }

    /// The ledger's tickets for a frame painted by `plan`.
    fn decide_backdrops(&mut self, plan: RenderPlan) -> Settled {
        let scene = self.ui.frame_scene();
        self.backdrop_captures.decide(
            scene.backdrops(),
            scene.stamp.display,
            BackdropDamage::of(plan, self.damage.dirty_backdrops()),
        )
    }

    /// Encode and compose the frame `plan` paints, under the ledger's tickets, its gradients in
    /// `rows`, and list every `GpuView` the window records as the backend's retention roster:
    /// recorded, not painted, so a culled unchanged view keeps its texture.
    fn build(&self, frontend: &mut Frontend, rows: &mut RowTable, plan: RenderPlan) {
        frontend.build(
            self.ui.frame_scene(),
            BuildPlan {
                render: plan,
                backdrops: self.backdrop_captures.tickets(),
                rows,
            },
        );
        self.ui
            .memory
            .gpu_views
            .collect_live_targets(&mut frontend.buffer.live_targets);
    }

    /// Whether a partial frame on `route` cannot paint in the target it keeps its pixels in: only a
    /// retained target paints a partial frame direct, and it cannot when it holds the last frame's
    /// damage outline, or a backdrop may capture from a target that gives no copy. Decided before
    /// the frame builds, from the tickets, as the partial draw list culls what a whole frame would
    /// paint.
    fn loses_kept_pixels(&self, route: Route) -> bool {
        let gives_copy = self.target.is_some_and(|target| target.gives_copy);
        route == Route::Direct
            && (self.target_outlined || (!gives_copy && self.backdrop_captures.may_capture()))
    }

    /// `path` for a frame whose backdrops capture as `buffer` says: a direct whole frame paints
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
    pub(crate) fn retire(&mut self, backend: &mut WgpuBackend) {
        backend.retire_stream(&mut self.stream);
    }

    /// The GPU half: draw `built` into `target`. Whatever `built` holds, the target then holds this
    /// frame, so the next frame's damage diffs against it.
    pub(crate) fn submit(
        &mut self,
        backend: &mut WgpuBackend,
        target: RenderTarget<'_>,
        built: BuiltFrame<'_>,
    ) {
        tracy::zone!();
        let size = target.size();
        let display_physical = self.ui.display().physical;
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
        let buffer = &frontend.buffer;
        let debug_overlay = self.ui.debug_overlay();
        self.target_outlined = debug_overlay.damage_rect;
        backend.submit(
            &mut self.stream,
            target,
            Submission {
                stats: &mut self.ui.host.gpu_pass_stats,
                store: &self.ui.forest.record_store,
                buffer,
                gpu_views: &self.ui.memory.gpu_views,
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
        let display = self.ui.display();
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
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::frame_report::FrameReport;
    use crate::ui::window_frames::WindowFrames;
    #[cfg(test)]
    use crate::ui::window_frames::present::present_facts::PaintPath;
    #[cfg(test)]
    use crate::ui::window_frames::present::{BuiltFrame, FrameSpec, TargetKey};
    #[cfg(test)]
    use std::time::Duration;

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
        /// What the frame's content changed, which leaves each backdrop whose footprint it touches
        /// stale. Kept apart from `damage`, whose rects may have merged past a backdrop no change
        /// reached.
        changed: Vec<Rect>,
    }

    impl WindowFrames {
        /// Have the next frame that paints paint `damage` over a change of `changed`, past the
        /// damage engine's answer and the present path's promotion, so a bench can time a decision
        /// production would not make, on a scene that holds still.
        pub(crate) fn force_next_frame(&mut self, damage: Damage, changed: &[Rect]) {
            self.forced.damage = Some(damage);
            self.forced.changed.clear();
            self.forced.changed.extend_from_slice(changed);
        }

        /// Put the forced decision in `report`'s plan, and mark the backdrops its change left
        /// stale.
        pub(crate) fn apply_forced(&mut self, report: &mut FrameReport) {
            let Some(damage) = self.forced.damage.take() else {
                return;
            };
            report.plan = Some(RenderPlan {
                clear: self.ui.theme().window_bg,
                damage,
            });
            // A whole frame captures every backdrop and reports none stale.
            if matches!(damage, Damage::Full) {
                return;
            }
            for backdrop in self.ui.frame_scene().backdrops() {
                if self
                    .forced
                    .changed
                    .iter()
                    .any(|rect| rect.intersects(backdrop.footprint))
                {
                    self.damage.force_stale_backdrop(backdrop.key);
                }
            }
        }

        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.stream.disable_cutout_tables();
        }

        /// Whether the target holds this window's last output; read by offscreen tests.
        #[cfg(test)]
        pub(crate) const fn output_valid(&self) -> bool {
            matches!(self.damage_base, DamageBase::Prev)
        }

        /// Whether a partial frame of this window painted a damage canvas.
        #[cfg(test)]
        pub(crate) const fn holds_canvas(&self) -> bool {
            self.stream.holds_canvas()
        }
    }
}

#[cfg(test)]
mod tests;
