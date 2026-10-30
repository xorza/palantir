//! [`WinitRuntime`]: the running windowed host.

use std::sync::Arc;
use std::time::Instant;

use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::app::App;
use crate::common::tracy;
use crate::gpu::error::SurfaceStartError;
use crate::gpu::surface::surface_manager::{FirstWindow, SurfaceManager, SurfaceStartup};
use crate::host::core::HostCore;
use crate::host::window_driver::PresentStrategy;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::handle::HostHandle;
use crate::host::winit::window::{FramePresent, Window};
use crate::host::winit::window_set::{WindowSet, WindowSlot};
use crate::host::winit::{Bootstrap, native};
use crate::text::font_scan::FontScan;
use crate::window::window_commands::{WindowCommand, WindowCommands};
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use std::fmt;
use std::mem;
use std::num::NonZeroU32;

pub(super) struct WinitRuntime<T> {
    /// The caller's app, created once the first window's `Ui` existed.
    pub(super) app: T,
    pub(super) surfaces: SurfaceManager,
    pub(super) core: HostCore,
    windows: WindowSet,
    pending_commands: WindowCommands,
    /// Each window's last frame, which the [`HostHandle`]s read.
    frames: Arc<FrameLog>,
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
        let options = &config.options;
        // Started before the window exists so the font scan overlaps window creation.
        let scan = options
            .shaper
            .is_none()
            .then(|| FontScan::spawn(options.fonts));
        let window = native::create_window(event_loop, token, &config.window)?;
        let SurfaceStartup {
            surfaces,
            gpu,
            first_surface,
        } = SurfaceManager::start(
            FirstWindow {
                display: event_loop.owned_display_handle(),
                window: &window,
                size: native::physical_size(&window),
            },
            config.gpu_config(),
        )
        .map_err(|error| match error {
            SurfaceStartError::Gpu { source } => WinitHostError::Gpu { source },
            SurfaceStartError::Surface { source } => WinitHostError::Surface { token, source },
        })?;
        let shaper =
            options.shaper_or_else(|| scan.expect("a scan runs when no shaper is set").join());
        let core = HostCore::new(gpu, options, shaper);
        let mut driver = core.open(token, PresentStrategy::DirectAdaptive);
        let create_app = bootstrap
            .create_app
            .take()
            .expect("bootstrap app factory already consumed");
        let pending_tasks = mem::take(&mut bootstrap.pending_tasks);

        let frames = Arc::clone(&handle.frames);
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
            frames,
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

    /// Publish whether the window at `slot` is hidden, for the handles to read.
    pub(super) fn log_visibility(&mut self, slot: WindowSlot) {
        let win = self.windows.at(slot);
        self.frames.set_hidden(win.driver.token, win.is_hidden());
    }

    /// Paint one window; it drains its commands into the pending queue. A lost device is replaced
    /// first ([`Self::recover`]).
    ///
    /// # Errors
    ///
    /// The device was lost and no new one opened, or the window's new surface failed.
    pub(super) fn draw(&mut self, slot: WindowSlot) -> Result<(), WinitHostError> {
        if self.core.gpu().is_lost() {
            self.recover()?;
        }
        let single_window = self.windows.len() == 1;
        let win = self.windows.at(slot);
        if let Some(summary) = win.frame(&mut self.core, &mut self.app, &mut self.pending_commands)
        {
            self.frames.record(win.driver.token, summary);
        }
        if win.surface.is_lost() {
            renew_surface(&self.surfaces, win, self.core.max_texture_dim());
        }
        if single_window {
            tracy::mark_main_frame();
        }
        Ok(())
    }

    /// Attach a new device in place of the lost one, through the steps startup took: the first
    /// window's new surface picks the adapter ([`SurfaceManager::reopen`]), the host core builds its
    /// device half on it and gives every window its stream there, and every other window gets a
    /// new surface; then every window paints whole.
    fn recover(&mut self) -> Result<(), WinitHostError> {
        let first = self.windows.iter_mut().next().expect("a window draws");
        let token = first.driver.token;
        let gpu = self
            .surfaces
            .reopen(&first.window, &mut first.surface)
            .map_err(|error| match error {
                SurfaceStartError::Gpu { source } => WinitHostError::DeviceLost { source },
                SurfaceStartError::Surface { source } => WinitHostError::Surface { token, source },
            })?;
        tracing::info!("graphics device lost: a new one is attached");
        self.core
            .attach(gpu, self.windows.iter_mut().map(|win| &mut win.driver));
        let max_texture_dim = self.core.max_texture_dim();
        for win in self.windows.iter_mut().skip(1) {
            renew_surface(&self.surfaces, win, max_texture_dim);
        }
        self.repaint_all();
        Ok(())
    }

    pub(super) fn repaint_all(&mut self) {
        for win in &mut self.windows {
            win.next = FramePresent::Immediate;
        }
    }

    /// Apply every window's queued opens and closes in order, in `about_to_wait`. A window that
    /// fails to open is logged and left closed: one failed secondary window does not stop the app.
    pub(super) fn drain_window_requests(&mut self, event_loop: &ActiveEventLoop) {
        // Taken out so the creates below can borrow `self`; put back to keep the buffer.
        let mut commands = mem::take(&mut self.pending_commands);
        for command in commands.drain() {
            match command {
                WindowCommand::Close(token) => self.close_window(token),
                WindowCommand::Open { token, config } => {
                    if let Err(error) = self.spawn_window(event_loop, token, &config) {
                        tracing::error!(?token, %error, "open_window failed");
                    }
                }
            }
        }
        self.pending_commands = commands;
    }

    /// Close every window for a quit: the loop exits once winit has destroyed them.
    pub(super) fn close_all_windows(&mut self) {
        while let Some(win) = self.windows.pop() {
            self.retire(win);
        }
    }

    pub(super) const fn has_no_windows(&self) -> bool {
        self.windows.is_empty()
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
            .make_surface(
                &window,
                SurfaceManager::clamp_extent(
                    self.core.max_texture_dim(),
                    native::physical_size(&window),
                ),
            )
            .map_err(|source| WinitHostError::Surface { token, source })?;
        let driver = self.core.open(token, PresentStrategy::DirectAdaptive);
        self.windows.push(Window::new(window, surface, driver));
        Ok(())
    }

    /// Tear down the window holding `token`, if any.
    fn close_window(&mut self, token: WindowToken) {
        if let Some(win) = self.windows.take(token) {
            self.retire(win);
        }
    }

    /// Tear down `win`: its render stream retires before the driver drops, and its surface goes
    /// before the native window, whose last share this must hold. A share held anywhere else keeps
    /// the window on screen past its close: on Wayland its toplevel goes only with the last one.
    fn retire(&mut self, win: Window) {
        let Window {
            window,
            surface,
            driver,
            ..
        } = win;
        self.frames.remove(driver.token);
        self.core.close(driver);
        drop(surface);
        debug_assert_eq!(
            Arc::strong_count(&window),
            1,
            "a closed window's native window is shared past its close"
        );
    }
}

/// Make `win` a new surface. On a failure the window keeps a lost one, and its next frame tries
/// again after a pause.
fn renew_surface(surfaces: &SurfaceManager, win: &mut Window, max_texture_dim: NonZeroU32) {
    if let Err(error) = surfaces.renew(&win.window, &mut win.surface, max_texture_dim) {
        tracing::error!(token = ?win.driver.token, %error, "window surface not made again");
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
