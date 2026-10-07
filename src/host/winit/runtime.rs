//! [`WinitRuntime`]: the running windowed host.

use std::time::Instant;

use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::common::tracy;
use crate::gpu::surface::surface_manager::{HostGpuConfig, SurfaceManager, SurfaceStartup};
use crate::host::core::{HostCore, HostCoreConfig};
use crate::host::window_driver::PresentStrategy;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::handle::HostHandle;
use crate::host::winit::window::{FramePresent, Window};
use crate::host::winit::window_set::{WindowSet, WindowSlot};
use crate::host::winit::{Bootstrap, native};
use crate::text::font_scan::FontScan;
use crate::window::window_commands::WindowCommands;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use std::fmt;
use std::mem;

pub(super) struct WinitRuntime<T> {
    /// The caller's app, created once the first window's `Ui` existed.
    pub(super) app: T,
    pub(super) surfaces: SurfaceManager,
    pub(super) core: HostCore,
    windows: WindowSet,
    pending_commands: WindowCommands,
}

impl<T> fmt::Debug for WinitRuntime<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WinitRuntime")
            .field("surfaces", &self.surfaces)
            .field("core", &self.core)
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

impl<T: App + 'static> WinitRuntime<T> {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        bootstrap: &mut Bootstrap<T>,
        handle: HostHandle<T>,
    ) -> Result<Self, WinitHostError> {
        let token = bootstrap.token;
        let config = &bootstrap.config;
        // Started before the window exists so the font scan overlaps window creation.
        let fonts = FontScan::spawn(config.fonts);
        let window = native::create_window(event_loop, token, &config.window)?;
        let SurfaceStartup {
            surfaces,
            first_surface,
        } = SurfaceManager::start(&window, native::physical_size(&window), gpu_config(config))
            .map_err(|source| WinitHostError::Surface { token, source })?;
        let core = HostCore::new(
            surfaces.gpu.clone(),
            surfaces.max_texture_dim,
            fonts.join(),
            Clipboard::system_or_memory(),
            HostCoreConfig {
                collect_gpu_stats: config.collect_gpu_stats,
                pixel_snap: config.pixel_snap,
            },
        );
        let mut driver = core
            .driver(token)
            .strategy(PresentStrategy::DirectAdaptive)
            .build();
        let create_app = bootstrap
            .create_app
            .take()
            .expect("bootstrap app factory already consumed");
        let pending_tasks = mem::take(&mut bootstrap.pending_tasks);

        let mut app = create_app(&mut driver.ui, handle);
        for task in pending_tasks {
            task(&mut app);
        }

        let mut windows = WindowSet::default();
        windows.push(Window::new(window, first_surface, driver));
        Ok(Self {
            app,
            surfaces,
            core,
            windows,
            pending_commands: WindowCommands::default(),
        })
    }

    /// Resolve the window winit reports events for as `id`, once per event.
    pub(super) fn slot_of_id(&self, id: WindowId) -> Option<WindowSlot> {
        self.windows.slot_of_id(id)
    }

    pub(super) fn window(&mut self, slot: WindowSlot) -> &mut Window {
        self.windows.at(slot)
    }

    pub(super) fn by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        self.windows.by_token(token)
    }

    /// Paint one window; it drains its commands into the pending queue.
    pub(super) fn draw(&mut self, slot: WindowSlot) {
        let single_window = self.windows.len() == 1;
        self.windows.at(slot).frame(
            &self.surfaces.gpu,
            &mut self.core,
            &mut self.app,
            &mut self.pending_commands,
        );
        if single_window {
            tracy::mark_main_frame();
        }
    }

    pub(super) fn repaint_all(&mut self) {
        for win in self.windows.iter_mut() {
            win.next = FramePresent::Immediate;
        }
    }

    /// Drain every window's open/close queue and apply it, in `about_to_wait`; requests are collected first so creates don't alias the pending list.
    pub(super) fn drain_window_requests(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), WinitHostError> {
        let mut commands = WindowCommands::default();
        commands.append(&mut self.pending_commands);
        // Closes first, so a same-frame close + open of one token recreates the window instead of tripping `spawn_window`'s duplicate-token guard.
        for token in commands.closes {
            self.close_window(token);
        }
        for pending in commands.opens {
            self.spawn_window(event_loop, pending.token, &pending.config)?;
        }
        if self.windows.is_empty() {
            event_loop.exit();
        }
        Ok(())
    }

    /// Repaint everything when an app-global setting changed (debug overlay flags, user scale).
    ///
    /// Both signals are taken before either is tested; `||` would short-circuit past the second and leave a stray repaint.
    pub(super) fn repaint_on_shared_change(&mut self) {
        let overlay = self.core.resources.diagnostics().overlay.take_change();
        let user_scale = self.core.resources.user_scale().take_change();
        if overlay || user_scale {
            self.repaint_all();
        }
    }

    /// Fold every window's [`FramePresent`] into one [`ControlFlow`]; the nearest deadline wins.
    pub(super) fn schedule(&self, event_loop: &ActiveEventLoop, now: Instant) {
        let earliest = earliest_wake(
            self.windows.iter().map(|win| (win, win.next.resolve(now))),
            |win| win.window.request_redraw(),
        );
        event_loop.set_control_flow(match earliest {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        token: WindowToken,
        config: &WindowConfig,
    ) -> Result<(), WinitHostError> {
        if self.windows.slot_of_token(token).is_some() {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return Ok(());
        }
        let window = native::create_window(event_loop, token, config)?;
        let surface = self
            .surfaces
            .make_surface(&window, native::physical_size(&window))
            .map_err(|source| WinitHostError::Surface { token, source })?;
        let driver = self
            .core
            .driver(token)
            .strategy(PresentStrategy::DirectAdaptive)
            .build();
        self.windows.push(Window::new(window, surface, driver));
        Ok(())
    }

    /// Tear down the window holding `token`, if any; the render stream retires before the driver drops.
    fn close_window(&mut self, token: WindowToken) {
        if let Some(win) = self.windows.take(token) {
            self.core.retire(&win.driver);
        }
    }
}

const fn gpu_config(config: &WinitHostConfig) -> HostGpuConfig {
    HostGpuConfig {
        power_preference: config.power_preference,
        vsync: config.vsync,
        collect_gpu_stats: config.collect_gpu_stats,
    }
}

/// The nearest future deadline among `presents`; windows wanting a frame now go to `redraw`.
fn earliest_wake<W>(
    presents: impl IntoIterator<Item = (W, FramePresent)>,
    mut redraw: impl FnMut(W),
) -> Option<Instant> {
    let mut earliest: Option<Instant> = None;
    for (window, present) in presents {
        match present {
            FramePresent::Immediate => redraw(window),
            FramePresent::At(at) => earliest = Some(earliest.map_or(at, |best| best.min(at))),
            FramePresent::Idle => {}
        }
    }
    earliest
}

#[cfg(test)]
mod tests {
    use crate::host::winit::runtime::earliest_wake;
    use crate::host::winit::window::FramePresent;
    use std::time::{Duration, Instant};

    #[test]
    fn the_nearest_deadline_wins_and_immediates_redraw() {
        let t0 = Instant::now();
        let at = |ms| FramePresent::At(t0 + Duration::from_millis(ms));
        for (label, presents, redrawn, wake) in [
            ("none", vec![], vec![], None),
            ("idle", vec![FramePresent::Idle], vec![], None),
            (
                "nearest of three",
                vec![at(30), at(10), at(20)],
                vec![],
                Some(t0 + Duration::from_millis(10)),
            ),
            (
                "mixed",
                vec![
                    FramePresent::Immediate,
                    at(40),
                    FramePresent::Idle,
                    FramePresent::Immediate,
                ],
                vec![0, 3],
                Some(t0 + Duration::from_millis(40)),
            ),
            (
                "all immediate",
                vec![FramePresent::Immediate; 2],
                vec![0, 1],
                None,
            ),
        ] {
            let mut got = Vec::new();
            let earliest = earliest_wake(presents.into_iter().enumerate(), |i| got.push(i));
            assert_eq!(got, redrawn, "{label}: redrawn");
            assert_eq!(earliest, wake, "{label}: wake");
        }
    }
}
