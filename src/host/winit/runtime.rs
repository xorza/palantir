//! [`WinitRuntime`]: the running windowed host.

use std::sync::Arc;
use std::time::Instant;

use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::common::tracy;
use crate::gpu::error::SurfaceStartError;
use crate::gpu::surface::surface_manager::{FirstWindow, SurfaceManager, SurfaceStartup};
use crate::host::core::HostCore;
use crate::host::system_preferences::SystemPreferences;
use crate::host::window_driver::PresentStrategy;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::handle::{HostHandle, UserEvent};
use crate::host::winit::recovery_backoff::RecoveryBackoff;
use crate::host::winit::remote::QueryTask;
use crate::host::winit::system_preferences;
use crate::host::winit::wake_latch::WakeLatch;
use crate::host::winit::window::{OpenedWindow, Window};
use crate::host::winit::window_set::{WindowSet, WindowSlot};
use crate::host::winit::{Bootstrap, native};
use crate::text::font_scan::FontScan;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::app::App;
use crate::window::waker::Waker;
use crate::window::window_commands::{WindowCommand, WindowCommands};
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use std::fmt;
use std::mem;
use std::num::NonZeroU32;

use mundy::Subscription;

pub(super) struct WinitRuntime<T: 'static> {
    /// The caller's app, created once the first window's `Ui` existed.
    pub(super) app: T,
    pub(super) surfaces: SurfaceManager,
    pub(super) core: HostCore,
    windows: WindowSet,
    pending_commands: WindowCommands,
    /// The failed attempts of a device recovery under way.
    recovery: RecoveryBackoff,
    /// Each window's last frame, which the [`HostHandle`]s read.
    frames: Arc<FrameLog>,
    /// The handle each window's [`Waker`] asks for its frames through.
    handle: HostHandle<T>,
    /// Follows the system's settings while the host runs; dropping it ends the following.
    _system_preferences: Subscription,
}

impl<T: 'static> fmt::Debug for WinitRuntime<T> {
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
        // Read while the font scan runs, so the first window opens in the system's scheme; the
        // first frame moves the decorations if the app's factory picks another.
        let system = system_preferences::read();
        let decorations = system.color_scheme.unwrap_or(ColorScheme::FALLBACK);
        let window = native::create_window(event_loop, token, &config.window, decorations)?;
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
        let mut core = HostCore::new(gpu, options, shaper);
        // Before the first `Ui`, so it starts in the system's scheme.
        let _ = system.apply(&core.cpu.resources);
        let mut driver = core.open(token, PresentStrategy::DirectAdaptive);
        driver.ui.set_waker(Self::waker(&handle, token));
        let create_app = bootstrap
            .create_app
            .take()
            .expect("bootstrap app factory already consumed");
        let pending_tasks = mem::take(&mut bootstrap.pending_tasks);

        let frames = Arc::clone(&handle.frames);
        let own_handle = handle.clone();
        let system_preferences = {
            let handle = handle.clone();
            // A host gone quiet drops the report, as it has no window left to show it in.
            system_preferences::subscribe(move |preferences| {
                handle.send(UserEvent::SystemPreferences(preferences)).ok();
            })
        };
        let mut app = create_app(&mut driver.ui, handle);
        for task in pending_tasks {
            task(&mut app);
        }

        let mut windows = WindowSet::default();
        windows.push(Window::new(OpenedWindow {
            window,
            surface: first_surface,
            driver,
            decorations,
            inner_size: config.window.inner_size,
        }));
        let runtime = Self {
            app,
            surfaces,
            core,
            windows,
            pending_commands: WindowCommands::default(),
            recovery: RecoveryBackoff::default(),
            frames,
            handle: own_handle,
            _system_preferences: system_preferences,
        };
        Ok(runtime)
    }

    /// The waker of the window `token`: a frame of it, through `handle`, one event in flight.
    fn waker(handle: &HostHandle<T>, token: WindowToken) -> Waker {
        let handle = handle.clone();
        let latch = WakeLatch::default();
        Waker::new(move || handle.request_repaint(token, &latch))
    }

    /// Adopt the system's settings as `mundy` reports a change; [`Self::repaint_on_shared_change`]
    /// then repaints every window if the scheme moved.
    pub(super) fn apply_system_preferences(&self, preferences: SystemPreferences) {
        let _ = preferences.apply(&self.core.cpu.resources);
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

    /// Run `task` with `&mut` the app and the `Ui` of `token`'s window, between two of its frames,
    /// and ask that window for a frame that records, as `task` may have changed the app; the
    /// frame counts the query as a sent event, so a settle after it waits for it. A window that is
    /// gone drops the task unrun.
    pub(super) fn query(&mut self, token: WindowToken, task: QueryTask<T>) {
        if let Some(win) = self.windows.by_token(token) {
            win.on_query(&mut self.app, task);
        }
    }

    /// Publish whether the window at `slot` is hidden, for the handles to read.
    pub(super) fn log_visibility(&mut self, slot: WindowSlot) {
        let win = self.windows.at(slot);
        self.frames.set_hidden(win.driver.token, win.is_hidden());
    }

    /// Paint one window; it drains its commands into the pending queue. A lost device is replaced
    /// first ([`Self::recover`]). While none holds, the frame runs the app and paints nothing, as
    /// an offscreen host's does, so a close request and its veto still run.
    ///
    /// # Errors
    ///
    /// No device opened and held in [`RecoveryBackoff`]'s window, or the window's new surface
    /// failed.
    pub(super) fn draw(&mut self, slot: WindowSlot) -> Result<(), WinitHostError> {
        if self.core.device.gpu().is_lost() {
            self.recover()?;
        }
        let single_window = self.windows.len() == 1;
        // A hidden window skips its frame and runs nothing of the app, so none follows it.
        self.windows.run(
            slot,
            Instant::now(),
            |win| {
                let ran = win.frame(&mut self.core, &mut self.app, &mut self.pending_commands)?;
                self.frames.record(win.driver.token, ran.summary);
                Some(ran.ran_app)
            },
            Instant::now,
        );
        let win = self.windows.at(slot);
        if self.core.device.gpu().is_lost() {
            // Lost before the frame or found by its submit or present: every window shows its last
            // frame, which no wake of its own replaces.
            self.wake_for_recovery();
        } else if win.surface.is_lost() {
            renew_surface(
                &self.surfaces,
                win,
                self.core.device.gpu().max_texture_dim(),
            );
        }
        if single_window {
            tracy::mark_main_frame();
        }
        Ok(())
    }

    /// Attach a new device, unless a failed attempt still backs off; on a success every window
    /// paints whole. A device lost again as it was attached counts as a failed attempt.
    fn recover(&mut self) -> Result<(), WinitHostError> {
        let now = Instant::now();
        if self.recovery.wait_left(now).is_some() {
            return Ok(());
        }
        let failure = match self.attach_new_device() {
            Ok(()) if !self.core.device.gpu().is_lost() => {
                tracing::info!("graphics device lost: a new one is attached");
                self.recovery.succeeded();
                self.follow_all();
                return Ok(());
            }
            Ok(()) => WinitHostError::DeviceKeptLost,
            Err(error) => error,
        };
        if self.recovery.exhausted(now) {
            return Err(failure);
        }
        tracing::warn!(%failure, "no new graphics device holds yet");
        self.recovery.failed(now);
        Ok(())
    }

    /// Wake every window when the next recovery attempt may run: at once, or once a failed one
    /// backed off.
    fn wake_for_recovery(&mut self) {
        let now = Instant::now();
        self.windows
            .hold_until(now + self.recovery.wait_left(now).unwrap_or_default());
    }

    /// Attach a new device in place of the lost one, through the steps startup took: the first
    /// window's new surface picks the adapter ([`SurfaceManager::reopen`]), the host core builds its
    /// device half on it and gives every window its stream there, and every other window gets a
    /// new surface.
    fn attach_new_device(&mut self) -> Result<(), WinitHostError> {
        let first = self.windows.iter_mut().next().expect("a window draws");
        let token = first.driver.token;
        let gpu = self
            .surfaces
            .reopen(&first.window, &mut first.surface)
            .map_err(|error| match error {
                SurfaceStartError::Gpu { source } => WinitHostError::DeviceLost { source },
                SurfaceStartError::Surface { source } => WinitHostError::Surface { token, source },
            })?;
        self.core.device.attach(
            gpu,
            &mut self.core.cpu,
            self.windows.iter_mut().map(|win| &mut win.driver),
        );
        let max_texture_dim = self.core.device.gpu().max_texture_dim();
        for win in self.windows.iter_mut().skip(1) {
            renew_surface(&self.surfaces, win, max_texture_dim);
        }
        Ok(())
    }

    /// Every window shows a change made outside the windows' frames: the app's, on the main
    /// thread; a setting; a new device. Each follows it, so no window's frame for it asks the
    /// others for another.
    pub(super) fn follow_all(&mut self) {
        self.windows.follow_all(Instant::now());
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

    /// Repaint everything when an app-global setting changed.
    pub(super) fn repaint_on_shared_change(&mut self) {
        if self.core.cpu.resources.settings().take_change() {
            self.follow_all();
        }
    }

    /// Fold every window's next frame into one [`ControlFlow`]; the nearest deadline wins.
    pub(super) fn schedule(&self, event_loop: &ActiveEventLoop, now: Instant) {
        event_loop.set_control_flow(match self.windows.wake(now) {
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
        let decorations = self.core.cpu.resources.color_scheme();
        let window = native::create_window(event_loop, token, config, decorations)?;
        let surface = self
            .surfaces
            .make_surface(
                &window,
                SurfaceManager::clamp_extent(
                    self.core.device.gpu().max_texture_dim(),
                    native::physical_size(&window),
                ),
            )
            .map_err(|source| WinitHostError::Surface { token, source })?;
        let mut driver = self.core.open(token, PresentStrategy::DirectAdaptive);
        driver.ui.set_waker(Self::waker(&self.handle, token));
        self.windows.push(Window::new(OpenedWindow {
            window,
            surface,
            driver,
            decorations,
            inner_size: config.inner_size,
        }));
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
        self.core.device.close(driver);
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
