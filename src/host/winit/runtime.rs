//! [`WinitRuntime`] — the windowed host once it is actually running: the
//! caller's app, the shared [`HostCore`] every window renders through, the
//! surface authority, and the live-window set.
//!
//! Almost nothing here is winit-specific; what is, is confined to the four
//! leaves the event loop owns — creating a window, requesting a redraw,
//! setting the control flow, and exiting. The rest (window registry, command
//! draining, diagnostics sync, present scheduling) is the same bookkeeping any
//! windowed host would do.

use std::sync::Arc;
use std::time::Instant;

use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window as WinitWindow, WindowId};

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::host::core::HostCore;
use crate::host::window_driver::WindowDriver;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::gpu::{GpuInit, SurfaceManager, WindowSurface};
use crate::host::winit::handle::HostHandle;
use crate::host::winit::window::{FramePresent, Window};
use crate::host::winit::{Bootstrap, native};
use crate::renderer::backend::BackendConfig;
use crate::text::TextShaper;
use crate::window::{WindowCommands, WindowConfig, WindowToken};

pub(super) struct WinitRuntime<T> {
    /// The caller's app, created once the first window's `Ui` existed.
    pub(super) app: T,
    /// Retained native-surface creation and presentation state.
    pub(super) surfaces: SurfaceManager,
    /// Shared resources, CPU frontend, and GPU backend — every window's `Ui`
    /// clones the first, and every window's frames run through the other two.
    pub(super) core: HostCore,
    observed_overlay: DebugOverlayConfig,
    /// Live windows in registration order. Lookups by either key are linear
    /// scans; window counts are tiny.
    windows: Vec<Window>,
    pending_commands: WindowCommands,
}

impl<T> std::fmt::Debug for WinitRuntime<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitRuntime")
            .field("surfaces", &self.surfaces)
            .field("core", &self.core)
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

fn window_clipboard() -> Clipboard {
    #[cfg(feature = "system-clipboard")]
    {
        Clipboard::system_or_memory()
    }
    #[cfg(not(feature = "system-clipboard"))]
    {
        Clipboard::default()
    }
}

impl<T: App + 'static> WinitRuntime<T> {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        bootstrap: &mut Bootstrap<T>,
        handle: HostHandle<T>,
    ) -> Result<Self, WinitHostError> {
        let token = bootstrap.token;
        let config = bootstrap.config.clone();
        let window = native::create_window(event_loop, token, &config.window)?;
        let GpuInit {
            surfaces,
            device,
            queue,
            first_surface,
        } = GpuInit::new(&window, &config)?;
        let core = HostCore::new(
            device,
            queue,
            TextShaper::new(),
            window_clipboard(),
            BackendConfig {
                collect_gpu_stats: config.collect_gpu_stats,
            },
        );
        let mut driver = core.driver(token).build();
        let create_app = bootstrap
            .create_app
            .take()
            .expect("bootstrap app factory already consumed");
        let pending_tasks = std::mem::take(&mut bootstrap.pending_tasks);

        let mut app = create_app(&mut driver.ui, handle);
        for task in pending_tasks {
            task(&mut app);
        }

        let observed_overlay = *core.shared.resources.diagnostics.overlay.borrow();
        Ok(Self {
            app,
            surfaces,
            core,
            observed_overlay,
            windows: vec![Window::new(window, first_surface, driver)],
            pending_commands: WindowCommands::default(),
        })
    }

    pub(super) fn by_id(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|win| win.window.id() == id)
    }

    pub(super) fn by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .find(|win| win.driver.token == token)
    }

    /// Paint one window; it stores its own schedule and drains its commands
    /// into the runtime's pending queue.
    pub(super) fn draw(&mut self, id: WindowId) {
        let Some(win) = self.windows.iter_mut().find(|win| win.window.id() == id) else {
            return;
        };
        win.frame(
            &self.surfaces,
            &mut self.core,
            &mut self.app,
            &mut self.pending_commands,
        );
    }

    pub(super) fn repaint_all(&mut self) {
        for win in &mut self.windows {
            win.next = FramePresent::Immediate;
        }
    }

    /// Drain every window's [`Ui::open_window`](crate::Ui::open_window) /
    /// [`Ui::close_window`](crate::Ui::close_window) queue and apply it. Runs
    /// in `about_to_wait`, the one callback that always holds
    /// `&ActiveEventLoop` after event processing. Requests are collected out
    /// of the pending queue *first* so the subsequent creates don't alias the
    /// list we're iterating.
    pub(super) fn drain_window_requests(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), WinitHostError> {
        let mut commands = WindowCommands::default();
        commands.append(&mut self.pending_commands);
        // Closes first, so a same-frame close + open of one token
        // recreates the window instead of tripping `spawn_window`'s
        // duplicate-token guard and losing it.
        for token in commands.closes {
            self.close_window(token);
        }
        for pending in commands.opens {
            self.spawn_window(event_loop, pending.token, pending.config)?;
        }
        if self.windows.is_empty() {
            // Every window closed (titlebar X or `close_window`) — nothing
            // left to drive.
            event_loop.exit();
        }
        Ok(())
    }

    /// Repaint everything when the app-global debug overlay changed, so a
    /// toggle in one window shows up in the others.
    pub(super) fn sync_diagnostics(&mut self) {
        let overlay = *self.core.shared.resources.diagnostics.overlay.borrow();
        if overlay != self.observed_overlay {
            self.observed_overlay = overlay;
            self.repaint_all();
        }
    }

    /// Fold every window's [`FramePresent`] into one [`ControlFlow`]. A window
    /// wanting `Immediate` (or a deadline already due) gets its own
    /// `request_redraw`; the loop wakes for it regardless of the `WaitUntil`.
    /// Future deadlines contribute their instant; the nearest wins so no
    /// window out-sleeps its own schedule.
    pub(super) fn schedule(&self, event_loop: &ActiveEventLoop, now: Instant) {
        let mut earliest: Option<Instant> = None;
        for win in &self.windows {
            match win.next.resolve(now) {
                FramePresent::Immediate => win.window.request_redraw(),
                FramePresent::At(at) => {
                    earliest = Some(earliest.map_or(at, |best: Instant| best.min(at)));
                }
                FramePresent::Idle => {}
            }
        }
        event_loop.set_control_flow(match earliest {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        token: WindowToken,
        config: WindowConfig,
    ) -> Result<(), WinitHostError> {
        if self.windows.iter().any(|win| win.driver.token == token) {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return Ok(());
        }
        let window = native::create_window(event_loop, token, &config)?;
        let surface = self.surfaces.make_surface(&window)?;
        let driver = self.core.driver(token).build();
        self.register_window(window, surface, driver);
        Ok(())
    }

    /// Tear down the window holding `token`; a no-op if none does. The render
    /// stream retires before the driver drops — see [`HostCore::retire`].
    fn close_window(&mut self, token: WindowToken) {
        let Some(index) = self
            .windows
            .iter()
            .position(|win| win.driver.token == token)
        else {
            return;
        };
        let win = self.windows.swap_remove(index);
        self.core.retire(&win.driver);
    }

    fn register_window(
        &mut self,
        window: Arc<WinitWindow>,
        surface: WindowSurface,
        driver: WindowDriver,
    ) {
        let id = window.id();
        assert!(
            self.windows.iter().all(|win| win.window.id() != id),
            "winit returned a duplicate WindowId"
        );
        self.windows.push(Window::new(window, surface, driver));
    }
}
