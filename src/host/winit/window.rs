//! Per-window winit state and swapchain frame orchestration.

use std::sync::Arc;
use std::time::Instant;

use glam::{IVec2, UVec2};
use winit::window::Window as WinitWindow;

use crate::Display;
use crate::app::App;
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, TargetKey, WindowDriver};
use crate::host::winit::gpu::{SurfaceManager, WindowSurface};
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
        #[cfg(feature = "profile-with-tracy")]
        let _tracy_frame = tracy_client::non_continuous_frame!("frame");
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
            self.finish(commands);
            return;
        }

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

        // A size or format change invalidates the driver's retained target
        // state *and* needs the swapchain reconfigured before the next
        // acquire. Identical repeats cost nothing (Wayland resends configures
        // on focus / output changes), which matters because
        // `surface.configure` waits for GPU idle and reallocates the
        // swapchain — wgpu #7447 measures 100ms+ stalls when called per
        // repeated event.
        if self.driver.note_target(TargetKey {
            physical,
            format: self.config.format,
        }) {
            surfaces.configure(&self.surface, &self.config);
        }

        let cpu = core.cpu_frame(&mut self.driver, display, app);
        self.next = self.present(surfaces, core, cpu);

        profiling::finish_frame!();
        self.finish(commands);
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
            match self.surface.get_current_texture() {
                Success(frame) => {
                    core.submit(&mut self.driver, &frame.texture, mode);
                    self.window.pre_present_notify();
                    surfaces.present(frame);
                    report.repaint_requested
                }
                Suboptimal(_) | Outdated | Lost => {
                    tracing::warn!("surface acquire: suboptimal / outdated / lost");
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
    /// window's own close command), push the requested cursor to the OS, and
    /// consume the one-shot close request.
    fn finish(&mut self, commands: &mut WindowCommands) {
        let cursor = self.driver.drain_window_output(commands);
        if cursor != self.cursor {
            self.window.set_cursor(native::cursor(cursor));
            self.cursor = cursor;
        }
        self.close_requested = false;
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
    use crate::text::TextShaper;
    use crate::window::{CursorIcon, WindowCommands, WindowConfig, WindowToken};

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

        let cursor = driver.drain_window_output(&mut commands);
        assert_eq!(cursor, CursorIcon::Pointer);
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
