//! [`WinitRuntime`] — the windowed host once it is actually running: the
//! caller's app, the shared [`HostCore`] every window renders through, the
//! surface authority, and the live-window set.
//!
//! Almost nothing here is winit-specific; what is, is confined to the four
//! leaves the event loop owns — creating a window, requesting a redraw,
//! setting the control flow, and exiting. The rest (window registry, command
//! draining, diagnostics sync, present scheduling) is the same bookkeeping any
//! windowed host would do.

use std::time::Instant;

use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::common::tracy;
use crate::host::core::{HostCore, HostCoreConfig};
use crate::host::winit::error::WinitHostError;
use crate::host::winit::gpu::{GpuInit, SurfaceManager};
use crate::host::winit::handle::HostHandle;
use crate::host::winit::window::{FramePresent, Window};
use crate::host::winit::window_set::{WindowSet, WindowSlot};
use crate::host::winit::{Bootstrap, native};
use crate::text::font_scan::FontScan;
use crate::window::window_commands::WindowCommands;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

pub(super) struct WinitRuntime<T> {
    /// The caller's app, created once the first window's `Ui` existed.
    pub(super) app: T,
    /// Retained native-surface creation and presentation state.
    pub(super) surfaces: SurfaceManager,
    /// Shared resources, CPU frontend, and GPU backend — every window's `Ui`
    /// clones the first, and every window's frames run through the other two.
    pub(super) core: HostCore,
    /// Live windows, addressed by either key through [`WindowSet`].
    windows: WindowSet,
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

impl<T: App + 'static> WinitRuntime<T> {
    pub(super) fn new(
        event_loop: &ActiveEventLoop,
        bootstrap: &mut Bootstrap<T>,
        handle: HostHandle<T>,
    ) -> Result<Self, WinitHostError> {
        let token = bootstrap.token;
        let config = bootstrap.config.clone();
        // Started before the window exists and joined below, so the font
        // scan overlaps window creation and GPU init rather than adding to
        // them. An early return leaves the thread to finish and drop.
        let fonts = FontScan::spawn(config.fonts);
        let window = native::create_window(event_loop, token, &config.window)?;
        let GpuInit {
            surfaces,
            first_surface,
        } = GpuInit::new(token, &window, &config)?;
        let core = HostCore::new(
            surfaces.device.clone(),
            surfaces.queue.clone(),
            surfaces.max_texture_dim,
            fonts.join(),
            Clipboard::system_or_memory(),
            HostCoreConfig {
                collect_gpu_stats: config.collect_gpu_stats,
                pixel_snap: config.pixel_snap,
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

    /// Resolve the window winit reports events for as `id`, once per
    /// event: the dispatch below acts on the slot rather than handing back
    /// a borrow, so a redraw does not scan the set a second time to find
    /// the window its caller has already found.
    pub(super) fn slot_of_id(&self, id: WindowId) -> Option<WindowSlot> {
        self.windows.slot_of_id(id)
    }

    pub(super) fn window(&mut self, slot: WindowSlot) -> &mut Window {
        self.windows.at(slot)
    }

    pub(super) fn by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        self.windows.by_token(token)
    }

    /// Paint one window; it stores its own schedule and drains its commands
    /// into the runtime's pending queue.
    pub(super) fn draw(&mut self, slot: WindowSlot) {
        let single_window = self.windows.len() == 1;
        self.windows.at(slot).frame(
            &self.surfaces,
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

    /// Repaint everything when an app-global setting changed, so a write in
    /// one window shows up in the others: the debug overlay's flags, and the
    /// user scale every window's `Display` is minted from.
    ///
    /// Both signals are taken before either is tested — `||` would short-
    /// circuit past the second, leaving its change to fire a stray repaint
    /// on whatever moved next.
    pub(super) fn repaint_on_shared_change(&mut self) {
        let overlay = self
            .core
            .shared
            .resources()
            .diagnostics()
            .overlay
            .take_change();
        let user_scale = self.core.shared.resources().user_scale().take_change();
        if overlay || user_scale {
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
        for win in self.windows.iter() {
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
        if self.windows.slot_of_token(token).is_some() {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return Ok(());
        }
        let window = native::create_window(event_loop, token, &config)?;
        let surface = self.surfaces.make_surface(token, &window)?;
        let driver = self.core.driver(token).build();
        self.windows.push(Window::new(window, surface, driver));
        Ok(())
    }

    /// Tear down the window holding `token`; a no-op if none does. The render
    /// stream retires before the driver drops — see [`HostCore::retire`].
    fn close_window(&mut self, token: WindowToken) {
        if let Some(win) = self.windows.take(token) {
            self.core.retire(&win.driver);
        }
    }
}
