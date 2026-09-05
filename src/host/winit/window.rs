//! Per-window winit state and swapchain frame orchestration.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{IVec2, UVec2, Vec2};
use winit::window::Window as WinitWindow;

use crate::app::App;
use crate::common::tracy::{self, FrameSet};
use crate::display;
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, TargetKey, WindowDriver};
use crate::host::winit::gpu::{self, SurfaceManager, WindowSurface};
use crate::host::winit::input::PointerTrace;
use crate::host::winit::native;
use crate::input::input_event::InputEvent;
use crate::input::response::input_delta::InputDelta;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_frame_state::WindowFrameState;
use crate::window::window_placement::WindowPlacement;

/// What only the windowing system can answer, held until an event that
/// can change it.
///
/// Each field is a round trip on X11 — `outer_position` an
/// `XTranslateCoordinates`, `is_maximized` a `get_property` — and
/// `current_monitor` additionally clones a handle that owns a `String`,
/// a heap allocation. Asking per frame paid all three for a reader that
/// asks only when an app calls
/// [`Ui::window_geometry`](crate::Ui::window_geometry), and for the
/// refresh rate the driver paces by.
///
/// Named apart from the [`WindowFrameState`] it half fills: that is what
/// the host *tells* the `Ui` each frame, this is what the host had to
/// *ask* for. [`Window::invalidate_system_facts`] names the events that
/// clear it.
#[derive(Clone, Copy, Debug)]
pub(super) struct SystemFacts {
    placement: WindowPlacement,
    refresh_millihertz: Option<u32>,
}

/// Where the pointer is, and the scale the recorder was last told it in.
///
/// The pair is one fact, not two: a physical position means nothing
/// without the divisor that turned it into the logical one the recorder
/// is holding, and comparing that divisor against the current scale is
/// the whole of how a stale pointer is noticed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PointerAnchor {
    physical: Vec2,
    /// The effective scale [`Window::effective_scale`] reported when this
    /// position was last delivered.
    scale: f32,
}

impl PointerAnchor {
    /// The anchor `trace` leaves behind, given the one `held` now and the
    /// `scale` the event was translated at.
    fn after(held: Option<Self>, trace: PointerTrace, scale: f32) -> Option<Self> {
        match trace {
            PointerTrace::Unchanged => held,
            PointerTrace::At(physical) => Some(Self { physical, scale }),
            PointerTrace::Gone => None,
        }
    }

    /// Adopt `scale` and return the logical position to re-tell the
    /// recorder — or `None` when the scale has not moved and what the
    /// recorder holds is still true.
    fn restate_at(&mut self, scale: f32) -> Option<Vec2> {
        if self.scale == scale {
            return None;
        }
        self.scale = scale;
        Some(self.physical / scale)
    }
}

/// First delay after a `Validation` acquire — about one frame at 60 Hz,
/// so a single spurious failure costs a dropped frame and nothing more.
const ACQUIRE_RETRY_MIN: Duration = Duration::from_millis(16);

/// Ceiling for [`Window::acquire_retry`]. A surface that stays invalid
/// settles at two wake-ups a second.
const ACQUIRE_RETRY_MAX: Duration = Duration::from_millis(500);

/// Everything one native window owns: its handle, swapchain state, target-
/// agnostic render driver, input/display facts, and event-loop schedule.
#[derive(Debug)]
pub(super) struct Window {
    pub(super) window: Arc<WinitWindow>,
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) driver: WindowDriver,
    /// The device pixel ratio winit reports for this window. The factor
    /// the UI is actually drawn and clicked at is
    /// [`Self::effective_scale`], which folds the app's own scale in.
    pub(super) system_scale: f32,
    pub(super) next: FramePresent,
    pub(super) close_requested: bool,
    cursor: CursorIcon,
    /// Time at which the window became hidden. The render core remains
    /// untouched while hidden, then its clock skips the elapsed gap on resume.
    occluded_at: Option<Instant>,
    /// This window's Tracy frame set. Zero-sized without the profiler.
    frame_set: FrameSet,
    /// See [`SystemFacts`]. `None` until the next frame asks.
    system_facts: Option<SystemFacts>,
    /// See [`PointerAnchor`]. `None` while the pointer is outside this
    /// window, where there is nothing to keep fresh.
    pointer: Option<PointerAnchor>,
    /// How long the next frame waits before retrying an acquire that
    /// failed validation — `None` while acquires are healthy.
    ///
    /// Every other acquire failure is transient: a timeout, an outdated
    /// swapchain, an occlusion. Repainting at once is the answer to
    /// those and the loop settles within a frame or two. A validation
    /// failure is the surface reporting that the call itself was wrong,
    /// and the next tick makes the same call — so without a delay the
    /// host builds a full CPU draw list per loop iteration for output
    /// nothing can accept. The delay doubles to [`ACQUIRE_RETRY_MAX`],
    /// which still picks a surface up within half a second of it
    /// becoming valid again.
    acquire_retry: Option<Duration>,
}

impl Window {
    pub(super) fn new(
        window: Arc<WinitWindow>,
        surface: WindowSurface,
        mut driver: WindowDriver,
    ) -> Self {
        let system_scale = display::sanitize_system_scale(window.scale_factor());
        // Seed the recorder's pacing level from the swapchain that was
        // actually opened, so `Ui::vsync` is truthful before any frame runs
        // and a control writing its own value back doesn't reconfigure an
        // explicitly-configured present mode out from under the host.
        driver
            .ui
            .seed_vsync(gpu::vsync_of(surface.config.present_mode));
        Self {
            window,
            surface: surface.surface,
            config: surface.config,
            driver,
            system_scale,
            next: FramePresent::Immediate,
            close_requested: false,
            cursor: CursorIcon::default(),
            occluded_at: None,
            frame_set: FrameSet::claim(),
            system_facts: None,
            pointer: None,
            acquire_retry: None,
        }
    }

    pub(super) fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.driver.ui.on_input(event)
    }

    /// Retain what an event said about the pointer, against the scale it
    /// was translated at.
    pub(super) fn note_pointer(&mut self, trace: PointerTrace, scale: f32) {
        self.pointer = PointerAnchor::after(self.pointer, trace, scale);
    }

    /// Re-tell the recorder where the pointer is when the effective scale
    /// has moved since it was last told.
    ///
    /// **The pointer is the one input that outlives its own event.** Every
    /// other one is consumed by the frame it arrived for, but a position
    /// stays true until the pointer moves again — which is what makes it
    /// the only one a scale change can leave stale. Nothing else re-sends
    /// it, so a monitor move or a [`Ui::set_user_scale`](crate::Ui::set_user_scale)
    /// write would otherwise hover and hit-test the new layout against a
    /// point from the old one, until the user happened to move the mouse.
    ///
    /// A no-op unless the scale actually moved, so the frame path pays one
    /// comparison. The injected event raises the input signal, and the
    /// frame this runs before is the one that acts on it.
    fn resync_pointer(&mut self) {
        let scale = self.effective_scale();
        if let Some(anchor) = &mut self.pointer
            && let Some(logical) = anchor.restate_at(scale)
        {
            self.driver.ui.on_input(InputEvent::PointerMoved(logical));
        }
    }

    /// Physical pixels per logical pixel *as the app sees them* — what a
    /// pointer position must be divided by to land in the space the frame
    /// laid its widgets out in.
    ///
    /// Read per event rather than cached beside [`Self::system_scale`]:
    /// the user scale is written from inside a frame, and a cached copy
    /// would hit-test the frame after that write against the scale before
    /// it.
    pub(super) fn effective_scale(&self) -> f32 {
        self.driver.ui.user_scale().applied_to(self.system_scale)
    }

    /// This frame's [`SystemFacts`], asking the windowing system only
    /// when an event has invalidated them.
    fn system_facts(&mut self) -> SystemFacts {
        if let Some(facts) = self.system_facts {
            return facts;
        }
        let facts = SystemFacts {
            placement: WindowPlacement {
                position: self
                    .window
                    .outer_position()
                    .ok()
                    .map(|position| IVec2::new(position.x, position.y)),
                maximized: self.window.is_maximized(),
            },
            refresh_millihertz: self
                .window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz()),
        };
        self.system_facts = Some(facts);
        facts
    }

    /// Drop the cached [`SystemFacts`].
    ///
    /// Called for every event that can move the window, resize it, or put
    /// it on another monitor — the three things the cached answers depend
    /// on. A superset is safe here and a missed event is not, so an event
    /// that merely *might* have changed one clears all three.
    pub(super) fn invalidate_system_facts(&mut self) {
        self.system_facts = None;
    }

    pub(super) fn set_occluded(&mut self, occluded: bool) {
        match (occluded, self.occluded_at) {
            (true, None) => self.occluded_at = Some(Instant::now()),
            (false, Some(at)) => {
                self.occluded_at = None;
                self.driver.clock.skip(at.elapsed());
            }
            _ => {}
        }
    }

    /// Run one application/UI frame, acquire and update the swapchain texture
    /// when needed, present it, then drain window-host output into `commands`
    /// and apply the cursor the frame asked for. Stores the resulting schedule
    /// on [`Self::next`].
    pub(super) fn frame<T: App>(
        &mut self,
        surfaces: &SurfaceManager,
        core: &mut HostCore,
        app: &mut T,
        commands: &mut WindowCommands,
    ) {
        tracy::zone!("Window::frame");

        let facts = self.system_facts();
        // Also where the previous frame's veto is asserted spent — see
        // `Ui::set_window_facts`. `finish` below sits outside the occlusion
        // branch, so every winit frame reaches the drain that clears it,
        // including a skipped one.
        self.driver.ui.set_window_facts(WindowFrameState {
            close_requested: self.close_requested,
            placement: facts.placement,
        });

        // An occluded window skips its frame, except the one carrying a
        // close request: `finish` below closes unless the app vetoed,
        // and the only place a veto can happen is inside `App::update` /
        // `App::record`. Skipping here would settle the close against a veto
        // flag no application code was ever offered — a minimized document
        // window would close straight past its "save changes?" prompt. The
        // request is one-shot, so this costs at most one frame per close.
        if self.occluded_at.is_some() && !self.close_requested {
            self.next = FramePresent::Idle;
        } else {
            // The close-request frame is the one that runs while
            // occluded, and `set_occluded(false)` skips the whole hidden
            // span on the premise that none did. Restarting the span here
            // is what keeps that premise true: otherwise a vetoed close
            // lets the un-occlude move the origin past the stamp this
            // frame already recorded, and the next `Clock::now` comes
            // back *earlier* than it. `advance_clock` saturates the `dt`
            // but still assigns `time`, so repaint deadlines and
            // multi-press timing would then be compared against a clock
            // that went backwards.
            if self.occluded_at.is_some() {
                self.occluded_at = Some(Instant::now());
            }
            // Before the display is minted, so a frame that lays out at a
            // new scale hit-tests against a pointer in the same space.
            self.resync_pointer();
            let physical = UVec2::new(self.config.width, self.config.height);
            let display =
                self.driver
                    .display(physical, self.system_scale, facts.refresh_millihertz);

            // A size, format, or present-mode change invalidates the driver's
            // retained target state *and* needs the swapchain reconfigured before
            // the next acquire. Identical repeats cost nothing (Wayland resends configures
            // on focus / output changes), which matters because
            // `surface.configure` waits for GPU idle and reallocates the
            // swapchain — wgpu #7447 measures 100ms+ stalls when called per
            // repeated event.
            if self.driver.note_target(TargetKey {
                physical,
                format: self.config.format,
                present_mode: Some(self.config.present_mode),
            }) {
                surfaces.configure(&self.surface, &self.config);
            }

            let cpu = core.cpu_frame(&mut self.driver, display, app);
            self.next = self.present(surfaces, core, cpu);
        }

        self.finish(commands);
        // This window's own frame set, not the main one: `Window::frame`
        // runs once per window per host-loop iteration, so marking the
        // main set here made Tracy's FPS readout tick N times per
        // iteration and report per-window slices as whole frames.
        // `WinitRuntime::draw` owns the main set.
        //
        // Past every exit, so an occluded frame closes its own Tracy
        // frame instead of being folded into the next painted one — the
        // difference between a minimized window reading as idle and it
        // reading as one multi-second frame.
        self.frame_set.mark();
    }

    /// Rebuild the swapchain and tell the driver that what it retained
    /// went with it.
    ///
    /// One step, because the images are new: the damage baseline and the
    /// last-frame pixels both describe contents that no longer exist. Two
    /// acquire arms reach here, and an arm that reconfigured without the
    /// second half was correct only by accident — see
    /// [`WindowDriver::invalidate_target_contents`].
    fn reconfigure(&mut self, surfaces: &SurfaceManager) {
        surfaces.configure(&self.surface, &self.config);
        self.driver.invalidate_target_contents();
    }

    fn present(
        &mut self,
        surfaces: &SurfaceManager,
        core: &mut HostCore,
        cpu: CpuFrame,
    ) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        let repaint = if report.plan.is_none() {
            // Nothing tried to acquire, so nothing can still be failing.
            self.acquire_retry = None;
            report.repaint_requested
        } else {
            let retry = self.acquire_retry.take();
            use wgpu::CurrentSurfaceTexture::*;
            // Bound before the match so the zone closes on acquire
            // rather than spanning the arm that submits: on a vsync-
            // paced present this call is where the frame blocks, and
            // folding the submit into it hides which of the two cost
            // the time.
            let acquired = {
                tracy::zone!("Surface::acquire");
                self.surface.get_current_texture()
            };
            match acquired {
                Success(frame) => {
                    core.submit(&mut self.driver, &frame.texture, mode);
                    self.window.pre_present_notify();
                    surfaces.present(frame);
                    report.repaint_requested
                }
                // Binding and dropping is what releases the acquired
                // texture here — `configure` fails with
                // `PreviousOutputExists` while one is still alive, and that
                // failure is a panic (wgpu surface configuration reports
                // through the device error sink).
                Suboptimal(frame) => {
                    tracing::warn!("surface acquire: suboptimal");
                    drop(frame);
                    self.reconfigure(surfaces);
                    true
                }
                Outdated | Lost => {
                    tracing::warn!("surface acquire: outdated / lost");
                    self.reconfigure(surfaces);
                    true
                }
                Timeout => {
                    tracing::warn!("surface acquire: timeout");
                    true
                }
                Validation => {
                    tracing::warn!("surface acquire: validation");
                    self.acquire_retry = Some(retry.map_or(ACQUIRE_RETRY_MIN, |delay| {
                        (delay * 2).min(ACQUIRE_RETRY_MAX)
                    }));
                    true
                }
                Occluded => false,
            }
        };

        // Ahead of `repaint`, which every failing acquire asks for: the
        // point of the delay is to pace exactly that request.
        if let Some(delay) = self.acquire_retry
            && let Some(at) = self.driver.clock.deadline(self.driver.clock.now() + delay)
        {
            return FramePresent::At(at);
        }
        if repaint {
            FramePresent::Immediate
        } else if let Some(at) = report
            .repaint_after
            .and_then(|duration| self.driver.clock.deadline(duration))
        {
            FramePresent::At(at)
        } else {
            FramePresent::Idle
        }
    }

    /// Settle everything the frame produced for the host: drain the recorder's
    /// window commands (which converts an un-vetoed close request into this
    /// window's own close command), push the requested cursor to the OS, apply
    /// a requested vsync change, and consume the one-shot close request.
    fn finish(&mut self, commands: &mut WindowCommands) {
        let output = self.driver.drain_window_output(commands);
        if output.cursor != self.cursor {
            self.window.set_cursor(native::cursor(output.cursor));
            self.cursor = output.cursor;
        }
        self.set_vsync(output.vsync);
        self.close_requested = false;
    }

    /// Point the swapchain config at `vsync`, if it isn't already paced that
    /// way. The comparison runs in [`Vsync`]'s two-state vocabulary rather
    /// than wgpu's: a window opened on an explicit `Mailbox` already *is*
    /// [`Vsync::Off`], so a recorder asking for `Off` must leave that finer
    /// choice standing rather than flatten it to `AutoNoVsync`.
    ///
    /// The reconfigure itself is left to the next frame's [`TargetKey`] check
    /// rather than done here. Recreating a swapchain invalidates the retained
    /// target state — the images are new, so the damage baseline and the
    /// last-frame pixels describe nothing — and doing it here would apply the
    /// new mode while `target` still named the old configuration, so the next
    /// key check would see no change and skip the reconfigure this asked for.
    /// (`WindowDriver::invalidate_target_contents` is what any configure owes
    /// the retained state, wherever it happens.)
    ///
    /// Hence the forced repaint: an idle window schedules no next frame, so
    /// without it the change would sit in `config` until something else
    /// happened to wake the window.
    fn set_vsync(&mut self, vsync: Vsync) {
        if gpu::vsync_of(self.config.present_mode) == vsync {
            return;
        }
        self.config.present_mode = gpu::present_mode(vsync);
        self.next = FramePresent::Immediate;
    }
}

/// Scheduling hint returned by a native-window frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

impl FramePresent {
    /// Collapse a deadline that has already come due into `Immediate`. A
    /// `WaitUntil` in the past fires instantly and spins the loop, so a window
    /// whose deadline passed may as well request its redraw now.
    pub(super) fn resolve(self, now: Instant) -> Self {
        match self {
            Self::At(t) if t <= now => Self::Immediate,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use glam::Vec2;

    use crate::common::clipboard::Clipboard;
    use crate::host::window_driver::WindowDriver;
    use crate::host::winit::input::PointerTrace;
    use crate::host::winit::window::{FramePresent, PointerAnchor};
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;
    use crate::ui::resources::UiResources;
    use crate::window::cursor_icon::CursorIcon;
    use crate::window::vsync::Vsync;
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_config::WindowConfig;
    use crate::window::window_token::WindowToken;

    const AT: Vec2 = Vec2::new(300.0, 120.0);

    fn anchor(scale: f32) -> Option<PointerAnchor> {
        PointerAnchor::after(None, PointerTrace::At(AT), scale)
    }

    #[test]
    fn a_trace_records_a_position_and_a_departure_clears_one() {
        let held = anchor(2.0);
        assert_eq!(held.map(|a| a.physical), Some(AT));

        assert_eq!(
            PointerAnchor::after(held, PointerTrace::Unchanged, 4.0),
            held,
            "an event carrying no position leaves the anchor alone, scale included",
        );
        assert_eq!(PointerAnchor::after(held, PointerTrace::Gone, 2.0), None);
        assert_eq!(
            PointerAnchor::after(None, PointerTrace::Unchanged, 2.0),
            None,
        );
    }

    /// The pointer sits at physical (300, 120). At scale 2 the recorder
    /// holds (150, 60); after a move to 2.5 it must hold (120, 48).
    #[test]
    fn a_scale_move_restates_the_position_once() {
        let mut held = anchor(2.0).unwrap();
        assert_eq!(held.restate_at(2.5), Some(Vec2::new(120.0, 48.0)));
        assert_eq!(
            held.restate_at(2.5),
            None,
            "the anchor adopted the scale, so the recorder is up to date",
        );
        assert_eq!(
            held.restate_at(2.0),
            Some(Vec2::new(150.0, 60.0)),
            "and a move back restates the position it started at",
        );
    }

    #[test]
    fn an_unmoved_scale_restates_nothing() {
        assert_eq!(anchor(1.5).unwrap().restate_at(1.5), None);
    }

    #[test]
    fn frame_drain_collects_commands_and_applies_close_veto() {
        let shared = UiResources::new(
            TextShaper::test_mono(),
            Clipboard::memory(),
            TextureLimit::default(),
        );
        let token = WindowToken(17);
        let mut driver = WindowDriver::builder(token, &shared, true).build();
        let opened = WindowToken(18);
        let mut commands = WindowCommands::default();

        driver
            .ui
            .open_window(opened, WindowConfig::new("inspector"));
        driver.ui.set_cursor(CursorIcon::Pointer);
        driver.ui.window_frame_mut().close_requested = true;

        let output = driver.drain_window_output(&mut commands);
        assert_eq!(output.cursor, CursorIcon::Pointer);
        assert_eq!(
            output.vsync,
            Vsync::On,
            "a frame that asked for nothing reports the standing level"
        );
        assert_eq!(commands.opens.len(), 1);
        assert_eq!(commands.opens[0].token, opened);
        assert_eq!(
            commands.closes,
            [token],
            "an un-vetoed close becomes this window's own close command"
        );
        assert!(driver.ui.window_requests().commands.opens.is_empty());
        assert!(driver.ui.window_requests().commands.closes.is_empty());
        // Drained by `append`, not `mem::take`, so the recorder keeps its
        // buffers for the next frame instead of reallocating per window
        // command.
        let open_capacity = driver.ui.window_requests().commands.opens.capacity();
        let close_capacity = driver.ui.window_requests().commands.closes.capacity();
        assert!(open_capacity > 0 && close_capacity > 0);

        driver.ui.window_frame_mut().close_requested = true;
        driver.ui.keep_open();
        let mut vetoed = WindowCommands::default();
        driver.drain_window_output(&mut vetoed);
        assert!(vetoed.closes.is_empty());

        // A second drain after the veto must not resurrect the request: the
        // frame state was consumed, so nothing is pending.
        let mut settled = WindowCommands::default();
        driver.drain_window_output(&mut settled);
        assert!(settled.closes.is_empty());
        assert!(!driver.ui.window_requests().close_vetoed);
        assert_eq!(
            driver.ui.window_requests().commands.opens.capacity(),
            open_capacity,
            "draining must not hand away the recorder's buffer"
        );
        assert_eq!(
            driver.ui.window_requests().commands.closes.capacity(),
            close_capacity
        );
    }

    /// Vsync is a level like the cursor, not a one-shot request: the drain
    /// copies it, it survives the drain that delivered it, and it reads back
    /// through `Ui::vsync` so an app never mirrors it. Collapsing a repeated
    /// level into no swapchain work is the host's job, not the recorder's —
    /// see `Window::set_vsync`.
    #[test]
    fn vsync_is_a_level_the_drain_copies_and_the_recorder_keeps() {
        let shared = UiResources::new(
            TextShaper::test_mono(),
            Clipboard::memory(),
            TextureLimit::default(),
        );
        let mut driver = WindowDriver::builder(WindowToken(3), &shared, true).build();
        let mut commands = WindowCommands::default();

        assert_eq!(driver.ui.vsync(), Vsync::On, "vsync is on unless asked off");
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::On);

        driver.ui.set_vsync(Vsync::Off);
        assert_eq!(driver.ui.vsync(), Vsync::Off, "the setter reads back");
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            Vsync::Off,
            "the level survives the drain that delivered it",
        );

        // Within one pass the last writer wins, matching `set_cursor`.
        driver.ui.set_vsync(Vsync::On);
        driver.ui.set_vsync(Vsync::Off);
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
    }

    #[test]
    fn due_deadlines_resolve_to_immediate_and_future_ones_stand() {
        let now = Instant::now();
        let past = now - Duration::from_millis(1);
        let future = now + Duration::from_millis(16);

        assert_eq!(FramePresent::At(past).resolve(now), FramePresent::Immediate);
        // `<=` — a deadline landing exactly on `now` is due, not pending.
        assert_eq!(FramePresent::At(now).resolve(now), FramePresent::Immediate);
        assert_eq!(
            FramePresent::At(future).resolve(now),
            FramePresent::At(future)
        );
        assert_eq!(
            FramePresent::Immediate.resolve(now),
            FramePresent::Immediate
        );
        assert_eq!(FramePresent::Idle.resolve(now), FramePresent::Idle);
    }
}
