//! Per-window winit state and swapchain frame orchestration.

use std::sync::Arc;
use std::time::Instant;

use glam::{IVec2, UVec2};
use winit::window::Window as WinitWindow;

use crate::Display;
use crate::app::App;
use crate::common::tracy::FrameSet;
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, TargetKey, WindowDriver};
use crate::host::winit::gpu::{self, SurfaceManager, WindowSurface};
use crate::host::winit::native;
use crate::input::InputEvent;
use crate::input::response::InputDelta;
use crate::window::{CursorIcon, WindowCommands, WindowFrameState};

/// Everything one native window owns: its handle, swapchain state, target-
/// agnostic render driver, input/display facts, and event-loop schedule.
#[derive(Debug)]
pub(super) struct Window {
    pub(super) window: Arc<WinitWindow>,
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) driver: WindowDriver,
    pub(super) scale_factor: f32,
    pub(super) next: FramePresent,
    pub(super) close_requested: bool,
    cursor: CursorIcon,
    /// Time at which the window became hidden. The render core remains
    /// untouched while hidden, then its clock skips the elapsed gap on resume.
    occluded_at: Option<Instant>,
    /// This window's Tracy frame set. Zero-sized without the profiler.
    frame_set: FrameSet,
}

impl Window {
    pub(super) fn new(
        window: Arc<WinitWindow>,
        surface: WindowSurface,
        driver: WindowDriver,
    ) -> Self {
        let scale_factor = window.scale_factor() as f32;
        Self {
            window,
            surface: surface.surface,
            config: surface.config,
            driver,
            scale_factor,
            next: FramePresent::Immediate,
            close_requested: false,
            cursor: CursorIcon::default(),
            occluded_at: None,
            frame_set: FrameSet::claim(),
        }
    }

    pub(super) fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.driver.ui.on_input(event)
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
        profiling::scope!("Window::frame");

        let position = self
            .window
            .outer_position()
            .ok()
            .map(|position| IVec2::new(position.x, position.y));
        self.driver.ui.window_frame = WindowFrameState {
            close_requested: self.close_requested,
            position,
            maximized: self.window.is_maximized(),
        };
        self.driver.ui.window_requests.close_vetoed = false;

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
            let physical = UVec2::new(self.config.width, self.config.height);
            let display = Display {
                physical,
                scale_factor: self.scale_factor,
                pixel_snap: self.driver.pixel_snap,
                refresh_millihertz: self
                    .window
                    .current_monitor()
                    .and_then(|monitor| monitor.refresh_rate_millihertz()),
            };

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

    fn present(
        &mut self,
        surfaces: &SurfaceManager,
        core: &mut HostCore,
        cpu: CpuFrame,
    ) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        let repaint = if report.plan.is_none() {
            report.repaint_requested
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            // Bound before the match so the zone closes on acquire
            // rather than spanning the arm that submits: on a vsync-
            // paced present this call is where the frame blocks, and
            // folding the submit into it hides which of the two cost
            // the time.
            let acquired = {
                profiling::scope!("Surface::acquire");
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
                    surfaces.configure(&self.surface, &self.config);
                    true
                }
                Outdated | Lost => {
                    tracing::warn!("surface acquire: outdated / lost");
                    surfaces.configure(&self.surface, &self.config);
                    true
                }
                Timeout | Validation => {
                    tracing::warn!("surface acquire: timeout / validation");
                    true
                }
                Occluded => false,
            }
        };

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
        if let Some(vsync) = output.vsync {
            self.set_present_mode(gpu::present_mode(vsync));
        }
        self.close_requested = false;
    }

    /// Point the swapchain config at `mode`. The reconfigure itself is left
    /// to the next frame's [`TargetKey`] check rather than done here: that
    /// check is the one gate on the retained target state, and recreating the
    /// swapchain invalidates it (the images are new, so the damage baseline
    /// and last-frame pixels describe nothing). Doing it here would reconfigure
    /// behind the gate's back and leave the next partial repaint loading
    /// contents that no longer exist.
    ///
    /// Hence the forced repaint: an idle window schedules no next frame, so
    /// without it the change would sit in `config` until something else
    /// happened to wake the window.
    fn set_present_mode(&mut self, mode: wgpu::PresentMode) {
        if mode == self.config.present_mode {
            return;
        }
        self.config.present_mode = mode;
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

    use crate::host::shared::HostShared;
    use crate::host::window_driver::WindowDriver;
    use crate::host::winit::window::FramePresent;
    use crate::text::shaper::TextShaper;
    use crate::window::{CursorIcon, Vsync, WindowCommands, WindowConfig, WindowToken};

    #[test]
    fn frame_drain_collects_commands_and_applies_close_veto() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let token = WindowToken(17);
        let mut driver = WindowDriver::builder(token, &shared).build();
        let opened = WindowToken(18);
        let mut commands = WindowCommands::default();

        driver
            .ui
            .open_window(opened, WindowConfig::new("inspector"));
        driver.ui.set_cursor(CursorIcon::Pointer);
        driver.ui.window_frame.close_requested = true;

        let output = driver.drain_window_output(&mut commands);
        assert_eq!(output.cursor, CursorIcon::Pointer);
        assert_eq!(output.vsync, None, "no vsync change was asked for");
        assert_eq!(commands.opens.len(), 1);
        assert_eq!(commands.opens[0].token, opened);
        assert_eq!(
            commands.closes,
            [token],
            "an un-vetoed close becomes this window's own close command"
        );
        assert!(driver.ui.window_requests.commands.opens.is_empty());
        assert!(driver.ui.window_requests.commands.closes.is_empty());
        // Drained by `append`, not `mem::take`, so the recorder keeps its
        // buffers for the next frame instead of reallocating per window
        // command.
        let open_capacity = driver.ui.window_requests.commands.opens.capacity();
        let close_capacity = driver.ui.window_requests.commands.closes.capacity();
        assert!(open_capacity > 0 && close_capacity > 0);

        driver.ui.window_frame.close_requested = true;
        driver.ui.keep_open();
        let mut vetoed = WindowCommands::default();
        driver.drain_window_output(&mut vetoed);
        assert!(vetoed.closes.is_empty());

        // A second drain after the veto must not resurrect the request: the
        // frame state was consumed, so nothing is pending.
        let mut settled = WindowCommands::default();
        driver.drain_window_output(&mut settled);
        assert!(settled.closes.is_empty());
        assert!(!driver.ui.window_requests.close_vetoed);
        assert_eq!(
            driver.ui.window_requests.commands.opens.capacity(),
            open_capacity,
            "draining must not hand away the recorder's buffer"
        );
        assert_eq!(
            driver.ui.window_requests.commands.closes.capacity(),
            close_capacity
        );
    }

    /// A vsync change is a one-shot request, not a per-frame value like the
    /// cursor: the drain **takes** it. Re-delivering would re-apply the same
    /// swapchain reconfigure — a GPU-idle wait and a fresh swapchain — on
    /// every subsequent frame.
    #[test]
    fn a_vsync_request_drains_once_and_then_clears() {
        let shared = HostShared::new(TextShaper::test_mono(), None);
        let mut driver = WindowDriver::builder(WindowToken(3), &shared).build();
        let mut commands = WindowCommands::default();

        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            None,
            "a frame that asked for nothing reports no change"
        );

        driver.ui.set_vsync(Vsync::Off);
        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            Some(Vsync::Off)
        );
        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            None,
            "the request is consumed by the drain that delivered it"
        );

        // Within one pass the last writer wins, matching `set_cursor`.
        driver.ui.set_vsync(Vsync::Off);
        driver.ui.set_vsync(Vsync::On);
        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            Some(Vsync::On)
        );
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
