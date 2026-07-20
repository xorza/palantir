//! Per-window winit state and swapchain frame orchestration.

use std::sync::Arc;
use std::time::Instant;

use glam::{IVec2, UVec2};
use winit::window::Window as WinitWindow;

use crate::Display;
use crate::app::App;
use crate::host::window_driver::{CpuFrame, WindowDriver};
use crate::host::winit::gpu::{SurfaceManager, WindowSurface};
use crate::input::InputEvent;
use crate::input::response::InputDelta;
use crate::renderer::backend::WgpuBackend;
use crate::window::{CursorIcon, WindowCommands, WindowFrameState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceTarget {
    physical: UVec2,
    format: wgpu::TextureFormat,
}

/// Everything one native window owns: its handle, swapchain state, target-
/// agnostic render driver, input/display facts, and event-loop schedule.
#[derive(Debug)]
pub(crate) struct Window {
    pub(crate) window: Arc<WinitWindow>,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) driver: WindowDriver,
    pub(crate) scale_factor: f32,
    pub(crate) next: FramePresent,
    pub(crate) close_requested: bool,
    pub(crate) cursor: CursorIcon,
    /// Last size and format applied with `surface.configure`. Configuration
    /// changes are coalesced until the next frame.
    configured: Option<SurfaceTarget>,
    /// Time at which the window became hidden. The render core remains
    /// untouched while hidden, then its clock skips the elapsed gap on resume.
    occluded_at: Option<Instant>,
}

impl Window {
    pub(crate) fn new(
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
            configured: None,
            occluded_at: None,
        }
    }

    pub(crate) fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.driver.ui.on_input(event)
    }

    pub(crate) fn set_occluded(&mut self, occluded: bool) {
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
    /// when needed, present it, then drain window-host output.
    pub(crate) fn frame<T: App>(
        &mut self,
        surfaces: &SurfaceManager,
        backend: &mut WgpuBackend,
        app: &mut T,
    ) -> WindowFrameOutput {
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

        if self.occluded_at.is_some() {
            return frame_output(&mut self.driver, FramePresent::Idle);
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

        let target = SurfaceTarget {
            physical,
            format: self.config.format,
        };
        if self.configured != Some(target) {
            self.driver.invalidate_target();
            surfaces.configure(&self.surface, &self.config);
            self.configured = Some(target);
        }

        let cpu = self.driver.cpu_frame(display, app);
        let present = self.present(surfaces, backend, cpu);

        profiling::finish_frame!();
        frame_output(&mut self.driver, present)
    }

    fn present(
        &mut self,
        surfaces: &SurfaceManager,
        backend: &mut WgpuBackend,
        cpu: CpuFrame,
    ) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        let repaint = if report.plan.is_none() {
            report.repaint_requested
        } else {
            use wgpu::CurrentSurfaceTexture::*;
            match self.surface.get_current_texture() {
                Success(frame) => {
                    self.driver.render_to_texture(backend, &frame.texture, mode);
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
}

fn frame_output(driver: &mut WindowDriver, present: FramePresent) -> WindowFrameOutput {
    let close_vetoed = driver.ui.window_requests.close_vetoed;
    if driver.ui.window_frame.close_requested && !close_vetoed {
        driver.ui.window_requests.commands.closes.push(driver.token);
    }
    let commands = std::mem::take(&mut driver.ui.window_requests.commands);
    driver.ui.window_frame = WindowFrameState::default();
    WindowFrameOutput {
        present,
        cursor: driver.ui.window_requests.cursor,
        commands,
    }
}

#[derive(Debug)]
pub(crate) struct WindowFrameOutput {
    pub(crate) present: FramePresent,
    pub(crate) cursor: CursorIcon,
    pub(crate) commands: WindowCommands,
}

/// Scheduling hint returned by a native-window frame.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

#[cfg(test)]
mod tests {
    use crate::host::shared::HostShared;
    use crate::host::window_driver::WindowDriver;
    use crate::host::winit::window::{FramePresent, frame_output};
    use crate::text::TextShaper;
    use crate::window::{CursorIcon, WindowConfig, WindowToken};

    #[test]
    fn frame_output_drains_commands_and_applies_close_veto() {
        let shared = HostShared::new(TextShaper::default());
        let token = WindowToken(17);
        let mut driver = WindowDriver::builder(token, &shared, 8192).build();
        let opened = WindowToken(18);

        driver
            .ui
            .open_window(opened, WindowConfig::new("inspector"));
        driver.ui.set_cursor(CursorIcon::Pointer);
        driver.ui.window_frame.close_requested = true;

        let output = frame_output(&mut driver, FramePresent::Idle);
        assert!(matches!(output.present, FramePresent::Idle));
        assert_eq!(output.cursor, CursorIcon::Pointer);
        assert_eq!(output.commands.opens.len(), 1);
        assert_eq!(output.commands.opens[0].token, opened);
        assert_eq!(output.commands.closes, [token]);
        assert!(driver.ui.window_requests.commands.opens.is_empty());
        assert!(driver.ui.window_requests.commands.closes.is_empty());

        driver.ui.window_frame.close_requested = true;
        driver.ui.keep_open();
        let vetoed = frame_output(&mut driver, FramePresent::Idle);
        assert!(vetoed.commands.closes.is_empty());
    }
}
