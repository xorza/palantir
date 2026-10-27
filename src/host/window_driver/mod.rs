//! `WindowDriver`: the target-agnostic state one host target owns around the
//! shared renderer: its [`Ui`], the persistent [`Backbuffer`] and the shadow cutout
//! plan. The host owns the time and hands it in with each [`FrameSpec`]. CPU scratch
//! lives on the shared [`Frontend`], GPU resources on the shared `WgpuBackend`, so N
//! windows share one renderer without sharing frame-local geometry.

use crate::app::App;
use crate::common::tracy;
use crate::damage::{Damage, FULL_REPAINT_THRESHOLD};
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::display::host_display::HostDisplay;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::submission::Submission;
use crate::gpu::frame::submission::SubmissionTargets;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutMode, CutoutPlan};
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::capture::Capture;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::RepaintScissors;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
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
    pixel_snap: bool,
    /// The target the last [`Self::note_target`] saw.
    target: Option<TargetKey>,
    cutouts: CutoutPlan,
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
}

/// What a host knows of the frame it is about to run: the target, the platform's scale and
/// refresh rate, and the time.
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameSpec {
    pub(super) physical: UVec2,
    pub(super) format: TargetFormat,
    pub(super) system_scale: f32,
    pub(super) refresh_millihertz: Option<u32>,
    pub(super) now: Duration,
}

impl FrameSpec {
    const fn target_key(&self) -> TargetKey {
        TargetKey {
            physical: self.physical,
            format: self.format,
        }
    }
}

/// What a host sets on each driver it opens.
#[derive(Clone, Copy, Debug)]
pub(super) struct DriverConfig {
    pub(super) strategy: PresentStrategy,
    /// The host's, inherited by every driver it opens.
    pub(super) pixel_snap: bool,
    /// How shadow corners are cut, as the backend allows ([`WgpuBackend::cutout_mode`]).
    pub(super) cutout_mode: CutoutMode,
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

/// Retires this driver's token from the
/// [`WindowDirectory`](crate::window::window_directory::WindowDirectory) on drop,
/// so no close path can leave `Ui::is_window_open` true.
impl Drop for WindowDriver {
    fn drop(&mut self) {
        self.ui.window_directory().remove(self.token);
    }
}

impl WindowDriver {
    /// A driver for `token` over the shared [`UiResources`]; registered in the window directory
    /// here, as its `Drop` retires it.
    pub(super) fn new(token: WindowToken, resources: &UiResources, config: DriverConfig) -> Self {
        resources.windows().add(token);
        Self {
            token,
            engines: FrameEngines::new(resources),
            ui: Ui::new(resources.clone()),
            render_owner: resources.render_owners().reserve(),
            backbuffer: None,
            backbuffer_fresh: false,
            output_valid: false,
            stencil: None,
            strategy: config.strategy,
            pixel_snap: config.pixel_snap,
            target: None,
            cutouts: CutoutPlan::new(config.cutout_mode),
            screenshot_requests: Vec::new(),
            captures: VecDeque::new(),
        }
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
        self.backbuffer_fresh = false;
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

    /// Hand the next frame the oldest screenshot the device has finished, or none: the image is
    /// an edge of exactly that frame.
    pub(super) fn deliver_screenshot(&mut self, backend: &WgpuBackend) {
        let landed = if self.captures.is_empty() {
            None
        } else {
            backend.poll();
            self.captures
                .front()
                .and_then(|pending| pending.capture.take())
        }
        .map(|image| {
            let pending = self
                .captures
                .pop_front()
                .expect("the screenshot that landed is the front one");
            Screenshot::new(image, pending.scale, pending.region, pending.id)
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
        let mut mode = path(report.plan, self.backbuffer_fresh);
        if !matches!(mode, PresentPath::SkipNoop) {
            self.output_valid = false;
        }
        if let PresentPath::Direct(plan) | PresentPath::ViaBackbuffer(plan) = mode {
            frontend.build(self.ui.frame_scene(), plan);
            if self.plan_cutouts(&frontend.buffer, plan) == Census::Stale {
                // The pixels this partial frame keeps show a cutout form the frame no longer picks.
                let full = plan.to_full();
                mode = path(Some(full), self.backbuffer_fresh);
                frontend.build(self.ui.frame_scene(), full);
                let census = self.plan_cutouts(&frontend.buffer, full);
                debug_assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
            }
        }
        // The host runs frames until each screenshot lands, as nothing else wakes it then.
        if capturing || !self.captures.is_empty() {
            report.repaint_requested = true;
        }
        CpuFrame { report, mode }
    }

    fn plan_cutouts(&mut self, buffer: &RenderBuffer, plan: RenderPlan) -> Census {
        let scissors = RepaintScissors::new(plan.damage, buffer);
        self.cutouts
            .build(&buffer.quads, &scissors, buffer.display.physical)
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
                let backbuffer = mode.renders_via_backbuffer().then(|| {
                    let ensured = backend.ensure_backbuffer(&mut self.backbuffer, target);
                    // A Partial arrives un-escalated only when `backbuffer_fresh`, so a recreate
                    // means that broke; the list was already Partial-culled, so escalating can't fix it.
                    debug_assert!(
                        !ensured.recreated || matches!(plan.damage, Damage::Full),
                        "backbuffer (re)created under a Partial plan whose draw \
                         list was culled for Partial"
                    );
                    ensured.backbuffer
                });
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
                let display = self.ui.display();
                for request in self.screenshot_requests.drain(..) {
                    let backbuffer = backbuffer
                        .expect("a frame with a screenshot request paints through the backbuffer");
                    let region = request.region.clamp_to(display.logical_rect());
                    // An ask made between frames, before the window shrank past its region, has
                    // no pixel left to copy: no image lands for it, as for an empty ask.
                    if region.is_paint_empty() {
                        continue;
                    }
                    self.captures.push_back(PendingScreenshot {
                        capture: backend.capture(backbuffer, physical_region(region, display)),
                        region,
                        scale: display.scale_factor(),
                        id: request.id,
                    });
                }
            }
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, CutoutPlan};
    use crate::host::window_driver::WindowDriver;

    impl WindowDriver {
        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.cutouts = CutoutPlan::new(CutoutMode::Shade);
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
