//! `WindowDriver`: the target-agnostic state one host target owns around the
//! shared renderer: its [`Ui`] and its [`RenderStream`], which keeps what its frames hold on the
//! device. The host owns the time and hands it in with each [`FrameSpec`]. CPU scratch
//! lives on the shared [`Frontend`], shared GPU resources on the `WgpuBackend`, so N
//! windows share one renderer without sharing frame-local geometry.

use crate::app::App;
use crate::common::tracy;
use crate::damage::{Damage, FULL_REPAINT_THRESHOLD};
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::display::host_display::HostDisplay;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::submission::{FrameOverlay, Route, Submission};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::Census;
use crate::gpu::surface::capture::Capture;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::BuildPlan;
use crate::renderer::frontend::Frontend;
use crate::renderer::pane_captures::{PaneCaptures, PaneDamage, Settled};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_input::FrameInput;
use crate::ui::frame_report::FrameReport;
use crate::ui::resources::UiResources;
use crate::window::screenshot::{Screenshot, ScreenshotId, ScreenshotRequest};
use crate::window::window_commands::WindowCommands;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::collections::VecDeque;
use std::thread;
use std::time::Duration;

/// Per-window state driving the host's shared [`Frontend`] and [`WgpuBackend`].
#[derive(Debug)]
pub(super) struct WindowDriver {
    pub(super) token: WindowToken,
    pub(super) ui: Ui,
    /// The layout / cascade / damage engines, held here out of authoring code's reach.
    engines: FrameEngines,
    /// What the window's frames keep on the device.
    stream: RenderStream,
    /// Whether the last frame completed its presentation. Invalid while a paint/copy
    /// is pending or after target invalidation, so the next frame drops its damage
    /// baseline.
    output_valid: bool,
    /// Whether the target holds the damage outline the last painted frame drew onto it, which a
    /// frame that keeps the target's pixels would keep too.
    target_outlined: bool,
    strategy: PresentStrategy,
    pixel_snap: bool,
    /// The target the last [`Self::note_target`] saw.
    target: Option<TargetKey>,
    /// The scissors the frame the CPU half built repaints inside, which its GPU half submits:
    /// worked out once, where an overflow can still make the frame whole.
    scissors: RepaintScissors,
    /// The captures this window's panes hold, which decide what a frame captures again.
    pane_captures: PaneCaptures,
    /// The images a recorded frame asked for, until a frame paints them: ones the host did not
    /// present (an outdated swapchain) carry to the next, and are resolved against the display of
    /// the frame that paints them, so a resize in between cannot leave one outside.
    screenshot_requests: Vec<ScreenshotRequest>,
    /// Screenshots on their way back from the device, oldest first; one lands per frame.
    captures: VecDeque<PendingScreenshot>,
}

/// The physical pixels of a logical `region` of `display`: edges rounded out, so every pixel
/// the region touches is in, and held inside the surface.
#[expect(
    clippy::cast_sign_loss,
    reason = "each coordinate is floored at zero first, and `as` saturates the far side"
)]
fn physical_region(region: Rect, display: Display) -> URect {
    let scale = display.scale_factor();
    let max = region.max();
    let bound = display.physical;
    let corner = |x: f32, y: f32| {
        UVec2::new(
            (x.max(0.0) as u32).min(bound.x),
            (y.max(0.0) as u32).min(bound.y),
        )
    };
    let min = corner(
        (region.min.x * scale).floor(),
        (region.min.y * scale).floor(),
    );
    let max = corner((max.x * scale).ceil(), (max.y * scale).ceil());
    URect::from_min_max(min, max.max(min))
}

/// A [`Capture`] with the request it answers.
#[derive(Debug)]
struct PendingScreenshot {
    capture: Capture,
    region: Rect,
    scale: f32,
    id: ScreenshotId,
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

/// What a host knows of the frame it is about to run: the target, the platform's scale and
/// refresh rate, and the time.
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameSpec {
    pub(super) physical: UVec2,
    pub(super) format: TargetFormat,
    /// Whether pixels can be copied out of the target, as a pane's capture is.
    pub(super) gives_copy: bool,
    pub(super) system_scale: f32,
    pub(super) refresh_millihertz: Option<u32>,
    pub(super) now: Duration,
}

impl FrameSpec {
    const fn target_key(&self) -> TargetKey {
        TargetKey {
            physical: self.physical,
            format: self.format,
            gives_copy: self.gives_copy,
        }
    }
}

/// What a host sets on each driver it opens.
#[derive(Clone, Copy, Debug)]
pub(super) struct DriverConfig {
    pub(super) strategy: PresentStrategy,
    /// The host's, inherited by every driver it opens.
    pub(super) pixel_snap: bool,
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
    /// One texture that keeps its pixels between frames: a painted frame paints into it, a
    /// partial one loading what it holds, and a still frame presents nothing. A frame that takes
    /// a screenshot, or a partial one that cannot keep the target's pixels as they are
    /// (`WindowDriver::kept_target_path`), paints through the backbuffer.
    Retained,
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
        PresentStrategy::Retained => plan.map_or(PresentPath::SkipNoop, PresentPath::Direct),
    }
}

/// The CPU half's result: the report plus the [`PresentPath`] sealed at
/// draw-list-build time.
#[derive(Debug)]
pub(super) struct CpuFrame {
    pub(super) report: FrameReport,
    pub(super) mode: PresentPath,
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
    /// A driver for `token` over the shared [`UiResources`], drawing through `stream`; registered
    /// in the window directory here, as its `Drop` retires it.
    pub(super) fn new(
        token: WindowToken,
        resources: &UiResources,
        config: DriverConfig,
        stream: RenderStream,
    ) -> Self {
        resources.windows().add(token);
        Self {
            token,
            engines: FrameEngines::new(resources),
            ui: Ui::new(resources.clone()),
            stream,
            output_valid: false,
            target_outlined: false,
            strategy: config.strategy,
            pixel_snap: config.pixel_snap,
            target: None,
            scissors: RepaintScissors::Full,
            pane_captures: PaneCaptures::default(),
            screenshot_requests: Vec::new(),
            captures: VecDeque::new(),
        }
    }

    /// The owner the window's frames render as.
    pub(super) const fn render_owner(&self) -> RenderOwnerId {
        self.stream.owner()
    }

    /// The stamp of the frame `spec` describes, as [`Self::cpu_frame`] takes it.
    pub(super) fn stamp(&self, spec: &FrameSpec) -> FrameStamp {
        let host = HostDisplay {
            physical: spec.physical,
            system_scale: spec.system_scale,
            pixel_snap: self.pixel_snap,
            refresh_millihertz: spec.refresh_millihertz,
        };
        self.ui.frame_stamp(host, spec.now)
    }

    /// Declare the target of the frame `spec` describes. A change invalidates retained pixels and
    /// the damage baseline.
    pub(super) fn note_target(&mut self, spec: &FrameSpec) {
        let key = spec.target_key();
        if self.target != Some(key) {
            self.target = Some(key);
            self.invalidate_target_contents();
        }
    }

    /// Declare the target's contents gone. Every swapchain rebuild owes this:
    /// [`Self::note_target`] makes it when the key moves, and the winit host when its
    /// swapchain rebuilds at the same key (new images: a lost, stale or repaced one).
    pub(super) const fn invalidate_target_contents(&mut self) {
        self.output_valid = false;
        self.stream.invalidate();
    }

    /// The shared CPU half: app lifecycle, record through damage, then the draw-list
    /// build if the frame paints. Seals the [`PresentPath`] (a promoted or resynced
    /// Partial builds a Full list).
    pub(super) fn cpu_frame<T: App>(
        &mut self,
        frontend: &mut Frontend,
        stamp: FrameStamp,
        app: &mut T,
    ) -> CpuFrame {
        tracy::zone!();
        let report = self.ui.frame(
            &mut self.engines,
            FrameInput::new(stamp, self.output_valid, self.token),
            app,
        );
        self.finish_cpu_frame(frontend, report)
    }

    /// Block until every screenshot on its way back has mapped, so the next frames land them.
    ///
    /// A wait for the device runs the map callbacks of the copies it finished, unless another
    /// thread's poll of the same device collected them first: wgpu then runs them on that thread,
    /// after its own wait, and this one returns before they ran. So the wait repeats until no
    /// copy is pending.
    pub(super) fn wait_for_captures(&self, gpu: &Gpu) {
        gpu.wait();
        while self
            .captures
            .iter()
            .any(|pending| pending.capture.is_pending())
        {
            thread::yield_now();
            gpu.wait();
        }
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
        if !self.captures.is_empty() {
            backend.poll();
        }
        let landed = self
            .captures
            .front()
            .is_some_and(|pending| !pending.capture.is_pending())
            .then(|| {
                let pending = self.captures.pop_front().expect("the front one landed");
                Screenshot::new(
                    pending.capture.take(),
                    pending.scale,
                    pending.region,
                    pending.id,
                )
            });
        self.ui.set_screenshot_taken(landed);
    }

    fn finish_cpu_frame(&mut self, frontend: &mut Frontend, mut report: FrameReport) -> CpuFrame {
        self.ui
            .take_screenshot_requests(&mut self.screenshot_requests);
        // A screenshot reads the backbuffer, so its frame paints all of it there, whatever
        // damage or present strategy the frame had.
        let capturing = !self.screenshot_requests.is_empty();
        if capturing {
            report.plan = Some(report.plan.map_or(
                RenderPlan {
                    clear: self.ui.theme().window_clear,
                    damage: Damage::Full,
                },
                RenderPlan::to_full,
            ));
        }
        let strategy = self.strategy;
        let path = |plan: Option<RenderPlan>, fresh: bool| match plan {
            Some(plan) if capturing => PresentPath::ViaBackbuffer(plan.to_full()),
            plan => present_path(plan, strategy, fresh),
        };
        let mut mode = path(report.plan, self.stream.is_backbuffer_fresh());
        if !matches!(mode, PresentPath::SkipNoop) {
            self.output_valid = false;
        }
        if let PresentPath::Direct(plan) | PresentPath::ViaBackbuffer(plan) = mode {
            // More panes capture than the repaint holds scissors for: the frame paints whole,
            // known before it builds.
            let mut plan = plan;
            if self.decide_panes(plan) == Settled::Overflow {
                plan = plan.to_full();
                mode = path(Some(plan), self.stream.is_backbuffer_fresh());
            }
            if let Some(through) = self.kept_target_path(mode) {
                mode = through;
                if let PresentPath::ViaBackbuffer(through) = through
                    && through != plan
                {
                    plan = through;
                    self.decide_panes(plan);
                }
            }
            self.build(frontend, plan);
            // The disjoint scissors outgrow their cap, or the pixels this partial frame keeps show
            // a cutout form the frame no longer picks: it paints whole.
            let scissors = RepaintScissors::new(plan.damage, &frontend.buffer);
            match scissors {
                Some(scissors) if self.plan_cutouts(&frontend.buffer, &scissors) == Census::Cur => {
                    self.scissors = scissors;
                }
                _ => {
                    let full = plan.to_full();
                    mode = path(Some(full), self.stream.is_backbuffer_fresh());
                    self.decide_panes(full);
                    self.build(frontend, full);
                    let census = self.plan_cutouts(&frontend.buffer, &RepaintScissors::Full);
                    debug_assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
                    self.scissors = RepaintScissors::Full;
                }
            }
            self.pane_captures.settle(&frontend.buffer);
            mode = self.capture_path(mode, &frontend.buffer);
        }
        // The host runs frames until each screenshot lands, as nothing else wakes it then.
        if capturing || !self.captures.is_empty() {
            report.repaint_requested = true;
        }
        CpuFrame { report, mode }
    }

    /// The ledger's tickets for a frame painted by `plan`.
    fn decide_panes(&mut self, plan: RenderPlan) -> Settled {
        let scene = self.ui.frame_scene();
        self.pane_captures.decide(
            scene.panes(),
            scene.stamp.display,
            PaneDamage::of(plan, self.engines.damage.dirty_panes()),
        )
    }

    /// Encode and compose the frame `plan` paints, under the ledger's tickets.
    fn build(&self, frontend: &mut Frontend, plan: RenderPlan) {
        frontend.build(
            self.ui.frame_scene(),
            BuildPlan {
                render: plan,
                panes: self.pane_captures.tickets(),
            },
        );
    }

    /// The path of a direct partial frame, which only a retained target takes, that cannot keep
    /// the target's pixels as they are: the target holds the last frame's damage outline, or a
    /// pane may capture from a target that gives no copy. Through the backbuffer, whole unless
    /// the backbuffer holds the last frame. Decided before the frame builds, from the tickets, as
    /// the partial draw list culls what a whole frame would paint.
    fn kept_target_path(&self, mode: PresentPath) -> Option<PresentPath> {
        let PresentPath::Direct(plan) = mode else {
            return None;
        };
        let partial = matches!(plan.damage, Damage::Partial(_));
        let gives_copy = self.target.is_some_and(|target| target.gives_copy);
        let uncopied = !gives_copy && self.pane_captures.may_capture();
        (partial && (self.target_outlined || uncopied)).then(|| {
            if self.stream.is_backbuffer_fresh() {
                PresentPath::ViaBackbuffer(plan)
            } else {
                PresentPath::ViaBackbuffer(plan.to_full())
            }
        })
    }

    /// `mode` for a frame whose panes capture as `buffer` says: a direct whole frame paints
    /// through the backbuffer when its captures cannot copy out of the target, which gives no
    /// copy, or would copy more than the surface holds, where its blurs read the backbuffer and
    /// the copy onto the target moves the surface once; the backbuffer may be new, as the frame
    /// is whole. A direct partial frame keeps its pixels in the target alone, so its captures copy
    /// out of it ([`Self::kept_target_path`]).
    fn capture_path(&self, mode: PresentPath, buffer: &RenderBuffer) -> PresentPath {
        let PresentPath::Direct(plan) = mode else {
            return mode;
        };
        let captured = buffer.captured_area();
        if captured == 0 {
            return mode;
        }
        let target = self
            .target
            .expect("a frame that captures has noted its target");
        if matches!(plan.damage, Damage::Partial(_)) {
            debug_assert!(
                target.gives_copy,
                "a partial frame's captures copy out of it"
            );
            return mode;
        }
        let surface = u64::from(target.physical.x) * u64::from(target.physical.y);
        if !target.gives_copy || captured >= surface {
            PresentPath::ViaBackbuffer(plan)
        } else {
            mode
        }
    }

    fn plan_cutouts(&mut self, buffer: &RenderBuffer, scissors: &RepaintScissors) -> Census {
        self.stream.plan_cutouts(buffer, scissors)
    }

    /// [`Ui::drain_window_output`] bound to this driver's token; the offscreen
    /// adapter drains into a dropped scratch buffer.
    pub(super) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        self.ui.drain_window_output(self.token, commands)
    }

    /// GPU submit against a caller-supplied texture, dispatching on the sealed
    /// [`PresentPath`]. [`PresentPath::SkipCopy`] copies the backbuffer onto the target.
    pub(super) fn render_to_texture(
        &mut self,
        buffer: &RenderBuffer,
        backend: &mut WgpuBackend,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        tracy::zone!();
        let size = target.size();
        let display_physical = self.ui.display().physical;
        debug_assert!(
            size == display_physical,
            "render_to_texture: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `cpu_frame` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `cpu_frame`.",
            size.x,
            size.y,
            display_physical.x,
            display_physical.y,
        );
        debug_assert!(
            self.target
                .is_some_and(|key| key.physical == size && key.format == target.format()),
            "render_to_texture: target ({}x{}, {:?}) differs from the one \
             `note_target` declared ({:?}), so the retained backbuffer / damage \
             baseline were never invalidated for it",
            size.x,
            size.y,
            target.format(),
            self.target,
        );
        debug_assert!(
            !matches!(mode, PresentPath::Direct(_))
                || buffer.captured_area() == 0
                || target.gives_copy(),
            "a direct frame's captures copy out of the target"
        );
        debug_assert!(
            self.strategy == PresentStrategy::Retained
                || !matches!(mode, PresentPath::Direct(plan) if matches!(plan.damage, Damage::Partial(_))),
            "a direct partial frame loads the target, which only a retained one keeps: a \
             swapchain image holds nothing at acquire"
        );
        let (plan, route) = match mode {
            PresentPath::SkipNoop => {
                self.output_valid = true;
                return;
            }
            PresentPath::SkipCopy => {
                // A `Skip` implies the previous frame painted at this size and format, so the
                // backbuffer exists and matches.
                backend.copy_backbuffer_to_surface(&self.stream, target);
                self.output_valid = true;
                self.target_outlined = false;
                return;
            }
            PresentPath::Direct(plan) => (plan, Route::Direct),
            PresentPath::ViaBackbuffer(plan) => (plan, Route::ViaBackbuffer),
        };
        debug_assert!(
            matches!(
                (plan.damage, &self.scissors),
                (Damage::Full, RepaintScissors::Full)
                    | (Damage::Partial(_), RepaintScissors::Partial(_))
            ),
            "the scissors are the ones the CPU half fitted to this plan"
        );
        let debug_overlay = self.ui.debug_overlay();
        self.target_outlined = debug_overlay.damage_rect;
        backend.submit(
            &mut self.stream,
            target,
            Submission {
                stats: self.ui.gpu_pass_stats(),
                store: self.ui.record_store(),
                buffer,
                clear: plan.clear,
                scissors: &self.scissors,
                route,
                overlay: FrameOverlay {
                    dim_undamaged: debug_overlay.dim_undamaged,
                    damage: debug_overlay.damage_rect.then_some(plan.damage),
                },
            },
        );
        self.output_valid = true;
        let display = self.ui.display();
        for request in self.screenshot_requests.drain(..) {
            debug_assert_eq!(
                route,
                Route::ViaBackbuffer,
                "a frame with a screenshot request paints through the backbuffer"
            );
            let region = request.region.clamp_to(display.logical_rect());
            // An ask made between frames, before the window shrank past its region, has
            // no pixel left to copy: no image lands for it, as for an empty ask.
            if region.is_paint_empty() {
                continue;
            }
            self.captures.push_back(PendingScreenshot {
                capture: backend.capture(&self.stream, physical_region(region, display)),
                region,
                scale: display.scale_factor(),
                id: request.id,
            });
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::host::window_driver::WindowDriver;

    impl WindowDriver {
        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.stream.disable_cutout_tables();
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
