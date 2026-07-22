//! `WinitHost` — wraps one-or-more [`WindowDriver`]s with winit
//! windows, their surfaces, and the [`ApplicationHandler`] event-loop
//! glue. Its lifecycle is encoded by [`HostPhase`]: bootstrap inputs become one
//! [`WinitRuntime`] containing the app, shared-resource root, surface manager,
//! backend, and complete live-window set. Per-window swapchain config, resize /
//! scale / occlusion handling, and `FramePresent` schedules fold across that
//! runtime into one `ControlFlow`.
//!
//! The caller-supplied app implements the [`App`] trait: [`App::update`]
//! runs once before a fully recorded frame, while [`App::record`] may replay
//! for cold-start warmup or relayout. The app is built by a closure handed to
//! [`WinitHostBuilder::build`], invoked once the first window's `Ui` +
//! [`HostHandle`] are ready (before the first frame) — so startup wiring
//! (theme tweaks, restoring persisted state, stashing the handle) happens
//! there.
//!
//! **Multi-window model.** Every window is an independent UI tree — its own
//! `Ui` (input / focus / layout / `Display`) and [`WindowDriver`] — all
//! rendering serially through one shared CPU [`Frontend`] and one shared
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The frontend reuses
//! encode/compose allocations between windows; the backend owns GPU renderer
//! resources. [`SurfaceManager`] retains the native-surface instance, adapter,
//! and cloned device/queue handles used to configure and present swapchains.
//! Windows are addressed by a caller-chosen [`WindowToken`]; winit's
//! opaque `WindowId` stays internal for event routing. The app opens /
//! closes windows from inside `record` via [`Ui::open_window`] /
//! [`Ui::close_window`].
//!
//! Submodules: [`config`] ([`WinitHostConfig`]), [`handle`]
//! ([`HostHandle`] + [`UserEvent`]), and [`gpu`] (surface/device startup).
//! The backend-agnostic window vocabulary ([`WindowToken`],
//! [`WindowConfig`]) lives in [`crate::window`].
//!
//! Usage:
//!
//! ```ignore
//! struct MyApp;
//! impl aperture::App for MyApp {
//!     fn record(&mut self, _win: WindowToken, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::builder(WindowToken(0))
//!     .title("title")
//!     .build(|ui, _handle| {
//!         ui.theme.button.anim = Some(AnimSpec::SPRING);
//!         MyApp
//!     })
//!     .run();
//! ```

pub(crate) mod config;
pub(crate) mod gpu;
pub(crate) mod handle;
mod input;
mod window;

use std::collections::HashMap;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use glam::IVec2;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Icon, Window as WinitWindow, WindowId};

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::host::shared::HostShared;
use crate::host::window_driver::WindowDriver;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::gpu::{GpuInit, SurfaceManager, WindowSurface};
use crate::host::winit::handle::{HostHandle, MainTask, UserEvent};
use crate::host::winit::window::{FramePresent, Window};
use crate::primitives::image::Image;
use crate::renderer::backend::{BackendConfig, WgpuBackend};
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::{CursorIcon, WindowCommands, WindowConfig, WindowToken};

type AppFactory<T> = Box<dyn FnOnce(&mut Ui, HostHandle<T>) -> T>;

/// Map the backend-agnostic cursor vocabulary onto winit's.
fn winit_cursor(cursor: CursorIcon) -> winit::window::CursorIcon {
    use winit::window::CursorIcon as W;
    match cursor {
        CursorIcon::Default => W::Default,
        CursorIcon::Pointer => W::Pointer,
        CursorIcon::Text => W::Text,
        CursorIcon::Grab => W::Grab,
        CursorIcon::Grabbing => W::Grabbing,
        CursorIcon::Move => W::Move,
        CursorIcon::Crosshair => W::Crosshair,
        CursorIcon::EwResize => W::EwResize,
        CursorIcon::NsResize => W::NsResize,
        CursorIcon::NotAllowed => W::NotAllowed,
    }
}

fn append_commands(target: &mut WindowCommands, source: &mut WindowCommands) {
    target.opens.append(&mut source.opens);
    target.closes.append(&mut source.closes);
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

/// What [`WinitHostBuilder::build`] stashes for the first `resumed`: the
/// bootstrap window's token + config and the caller's app factory. Consumed —
/// winit hands out `&ActiveEventLoop` only inside callbacks, so window +
/// GPU + app construction all wait here until then.
struct Bootstrap<T: 'static> {
    token: WindowToken,
    config: WinitHostConfig,
    create_app: Option<AppFactory<T>>,
    pending_tasks: Vec<MainTask<T>>,
}

struct WinitRuntime<T> {
    /// The caller's app, created once the first window's `Ui` existed.
    app: T,
    /// Retained native-surface creation and presentation state.
    surfaces: SurfaceManager,
    /// Shared, app-global state (render handles + live-window set + debug
    /// overlay) every window's `Ui` clones; each `WindowDriver` and the
    /// backend (render handles only) derive from it.
    shared: HostShared,
    /// Shared CPU encode/compose allocations, reused serially across windows.
    frontend: Frontend,
    /// The one shared GPU renderer every window draws through (pipelines,
    /// atlases); passed into each window's native frame adapter.
    backend: WgpuBackend,
    observed_overlay: DebugOverlayConfig,
    windows: HashMap<WindowId, Window>,
    pending_commands: WindowCommands,
}

enum HostPhase<T: 'static> {
    Bootstrap(Bootstrap<T>),
    Running(Box<WinitRuntime<T>>),
}

impl<T: 'static> std::fmt::Debug for Bootstrap<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bootstrap")
            .field("token", &self.token)
            .field("config", &self.config)
            .field("create_app", &self.create_app.is_some())
            .field("pending_tasks", &self.pending_tasks.len())
            .finish()
    }
}

impl<T> std::fmt::Debug for WinitRuntime<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitRuntime")
            .field("surfaces", &self.surfaces)
            .field("shared", &self.shared)
            .field("backend", &self.backend)
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

impl<T> WinitRuntime<T> {
    fn new(
        event_loop: &ActiveEventLoop,
        bootstrap: &mut Bootstrap<T>,
        handle: HostHandle<T>,
    ) -> Self {
        let token = bootstrap.token;
        let config = bootstrap.config.clone();
        let create_app = bootstrap
            .create_app
            .take()
            .expect("bootstrap app factory already consumed");
        let pending_tasks = std::mem::take(&mut bootstrap.pending_tasks);
        let window = create_window(event_loop, &config.window);
        let GpuInit {
            surfaces,
            device,
            queue,
            first_surface,
        } = GpuInit::new(&window, &config);
        let max_texture_dimension_2d = NonZeroU32::new(surfaces.max_texture_dim)
            .expect("device texture dimension limit is zero");
        let shared = HostShared::with_clipboard(
            TextShaper::with_bundled_fonts(),
            window_clipboard(),
            Some(max_texture_dimension_2d),
        );
        let backend = WgpuBackend::new(
            device,
            queue,
            shared.backend_resources(),
            BackendConfig {
                collect_gpu_stats: config.collect_gpu_stats,
            },
        );
        let frontend = Frontend::new(surfaces.max_texture_dim, shared.gradient_atlas.clone());
        let mut driver = WindowDriver::builder(token, &shared).build();

        shared.resources.windows.set_live(token, true);
        let mut app = create_app(&mut driver.ui, handle);
        for task in pending_tasks {
            task(&mut app);
        }

        let id = window.id();
        let windows = HashMap::from([(id, Window::new(window, first_surface, driver))]);
        let observed_overlay = *shared.resources.diagnostics.overlay.borrow();
        Self {
            app,
            surfaces,
            shared,
            frontend,
            backend,
            observed_overlay,
            windows,
            pending_commands: WindowCommands::default(),
        }
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        token: WindowToken,
        config: WindowConfig,
    ) {
        if self
            .windows
            .values()
            .any(|state| state.driver.token == token)
        {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return;
        }
        let window = create_window(event_loop, &config);
        let surface = self.surfaces.make_surface(&window);
        let driver = WindowDriver::builder(token, &self.shared).build();
        self.register_window(window, surface, driver);
    }

    fn register_window(
        &mut self,
        window: Arc<WinitWindow>,
        surface: WindowSurface,
        driver: WindowDriver,
    ) {
        let id = window.id();
        self.shared.resources.windows.set_live(driver.token, true);
        let previous = self
            .windows
            .insert(id, Window::new(window, surface, driver));
        assert!(previous.is_none(), "winit returned a duplicate WindowId");
    }
}

impl<T: 'static> std::fmt::Debug for HostPhase<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bootstrap(bootstrap) => f.debug_tuple("Bootstrap").field(bootstrap).finish(),
            Self::Running(runtime) => f.debug_tuple("Running").field(runtime).finish(),
        }
    }
}

/// Top-level winit-driven aperture runtime. Owns the caller-supplied app
/// `T: App` (RAII lifetime, no `Rc<RefCell<>>` to manage) and calls its
/// update/record lifecycle once per redraw, per window. `HostPhase` makes
/// bootstrap and running ownership mutually exclusive.
pub struct WinitHost<T: 'static> {
    phase: HostPhase<T>,
    event_loop: Option<EventLoop<UserEvent<T>>>,
    proxy: EventLoopProxy<UserEvent<T>>,
}

impl<T: 'static> std::fmt::Debug for WinitHost<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitHost")
            .field("phase", &self.phase)
            .field("event_loop", &self.event_loop.is_some())
            .finish_non_exhaustive()
    }
}

/// Startup configuration for [`WinitHost`].
#[derive(Debug)]
pub struct WinitHostBuilder<T> {
    first_token: WindowToken,
    config: WinitHostConfig,
    marker: PhantomData<fn() -> T>,
}

impl<T> WinitHostBuilder<T>
where
    T: App + 'static,
{
    /// Replace all startup tunables at once. Granular setters called afterward
    /// override individual fields.
    pub fn config(mut self, config: WinitHostConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the bootstrap window's full configuration.
    pub fn window(mut self, window: WindowConfig) -> Self {
        self.config.window = window;
        self
    }

    /// Set the bootstrap window's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.config.window.title = title.into();
        self
    }

    /// Set the app-global presentation policy. An explicit mode unsupported by
    /// a surface falls back to its matching automatic policy.
    pub fn present_mode(mut self, mode: wgpu::PresentMode) -> Self {
        self.config.present_mode = mode;
        self
    }

    /// Set the adapter power preference used at startup.
    pub fn power_preference(mut self, pref: wgpu::PowerPreference) -> Self {
        self.config.power_preference = pref;
        self
    }

    /// Opt into GPU timestamp and pipeline-statistics collection.
    pub fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.config.collect_gpu_stats = collect;
        self
    }

    /// Create the event loop and runtime host. `create_app` remains deferred
    /// until winit provides the first active event-loop callback and its `Ui`.
    pub fn build(
        self,
        create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static,
    ) -> WinitHost<T> {
        // EventLoop is built up front so `handle()` can hand out a proxy
        // before `run()` is called — that's the whole point of letting
        // threads spawn knowing where to send their pokes.
        let mut event_loop_builder = EventLoop::<UserEvent<T>>::with_user_event();
        // winit installs a default macOS menu whose Quit item binds ⌘Q to
        // `terminate:`, which kills the process before the event loop can
        // hand the app a `CloseRequested` to veto (save-on-exit prompts).
        // Drop that menu so ⌘Q arrives as an ordinary key event the app
        // handles like any other quit request.
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            event_loop_builder.with_default_menu(false);
        }
        let event_loop = event_loop_builder.build().expect("event loop");
        let proxy = event_loop.create_proxy();
        WinitHost {
            phase: HostPhase::Bootstrap(Bootstrap {
                token: self.first_token,
                config: self.config,
                create_app: Some(Box::new(create_app)),
                pending_tasks: Vec::new(),
            }),
            event_loop: Some(event_loop),
            proxy,
        }
    }
}

impl<T> WinitHost<T>
where
    T: App + 'static,
{
    /// Start configuring a winit-driven host whose bootstrap window is
    /// addressed by `first_token`.
    pub fn builder(first_token: WindowToken) -> WinitHostBuilder<T> {
        WinitHostBuilder {
            first_token,
            config: WinitHostConfig::default(),
            marker: PhantomData,
        }
    }

    /// Return a cheap-to-clone, `Send` handle for cross-thread repaint
    /// requests and run-on-main scheduling. Stable for the lifetime of
    /// the host — call before `run()` and ship the handle to worker
    /// threads.
    pub fn handle(&self) -> HostHandle<T> {
        HostHandle {
            proxy: self.proxy.clone(),
        }
    }

    /// Drive the (already-constructed) event loop to completion.
    pub fn run(mut self) {
        let event_loop = self.event_loop.take().expect("event loop already consumed");
        event_loop.run_app(&mut self).expect("run app");
    }

    /// Find the window addressed by a caller token (linear scan — window
    /// counts are tiny). `None` if no live window carries it.
    fn window_by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return None;
        };
        runtime
            .windows
            .values_mut()
            .find(|w| w.driver.token == token)
    }

    /// Paint one window and store the returned schedule back on it.
    fn draw(&mut self, id: WindowId) {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return;
        };
        let Some(win) = runtime.windows.get_mut(&id) else {
            return;
        };
        let mut output = win.frame(
            &runtime.surfaces,
            &mut runtime.frontend,
            &mut runtime.backend,
            &mut runtime.app,
        );
        win.next = output.present;
        if output.cursor != win.cursor {
            win.window.set_cursor(winit_cursor(output.cursor));
            win.cursor = output.cursor;
        }
        win.close_requested = false;
        append_commands(&mut runtime.pending_commands, &mut output.commands);
    }

    /// Drain every window's [`Ui::open_window`] / [`Ui::close_window`]
    /// queues and apply them. Runs in `about_to_wait`, the one callback
    /// that always holds `&ActiveEventLoop` after event processing.
    /// Requests are collected out of the per-window queues *first* so the
    /// subsequent `create_window` inserts don't alias the map we're
    /// iterating.
    fn drain_window_requests(&mut self, event_loop: &ActiveEventLoop) {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return;
        };
        let mut commands = WindowCommands::default();
        append_commands(&mut commands, &mut runtime.pending_commands);
        // Closes first, so a same-frame close + open of one token
        // recreates the window instead of tripping `spawn_window`'s
        // duplicate-token guard and losing it.
        for token in commands.closes {
            if runtime
                .windows
                .values()
                .any(|win| win.driver.token == token)
            {
                runtime.windows.retain(|_, win| win.driver.token != token);
                runtime.shared.resources.windows.set_live(token, false);
            }
        }
        for pw in commands.opens {
            runtime.spawn_window(event_loop, pw.token, pw.config);
        }
        if runtime.windows.is_empty() {
            // Every window closed (titlebar X or `close_window`) — nothing
            // left to drive.
            event_loop.exit();
        }
    }

    fn sync_diagnostics(&mut self) {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return;
        };
        let overlay = *runtime.shared.resources.diagnostics.overlay.borrow();
        if overlay != runtime.observed_overlay {
            runtime.observed_overlay = overlay;
            for win in runtime.windows.values_mut() {
                win.next = FramePresent::Immediate;
            }
        }
    }
}

/// Build a winit `Window` from a [`WindowConfig`]. Free fn (not a
/// method) so it borrows neither `self` nor the shared `Gpu`. Converts
/// the backend-agnostic logical `UVec2` sizes into winit `LogicalSize`
/// here so the winit type stays internal.
fn create_window(event_loop: &ActiveEventLoop, cfg: &WindowConfig) -> Arc<WinitWindow> {
    let mut attrs = WinitWindow::default_attributes()
        .with_title(cfg.title.clone())
        .with_maximized(cfg.maximized);
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(icon) = &cfg.icon {
        attrs = attrs.with_window_icon(Some(platform_icon(icon)));
    }
    // Restore a saved position only if it still lands on a connected
    // monitor — winit does no such clamping, so a window saved on a
    // since-disconnected display would otherwise reopen off-screen and
    // unreachable.
    if let Some(p) = cfg.position
        && position_on_monitor(event_loop, p)
    {
        attrs = attrs.with_position(PhysicalPosition::new(p.x, p.y));
    }
    Arc::new(event_loop.create_window(attrs).expect("create window"))
}

fn platform_icon(icon: &Image) -> Icon {
    Icon::from_rgba(icon.pixels.clone(), icon.size.x, icon.size.y)
        .expect("validated Image rejected by winit")
}

/// Whether `pos` (physical, window top-left) falls inside any currently
/// connected monitor's bounds — the guard that keeps a restored position
/// from placing the window off every screen.
fn position_on_monitor(event_loop: &ActiveEventLoop, pos: IVec2) -> bool {
    event_loop.available_monitors().any(|m| {
        let mp = m.position();
        let ms = m.size();
        pos.x >= mp.x
            && pos.y >= mp.y
            && pos.x < mp.x + ms.width as i32
            && pos.y < mp.y + ms.height as i32
    })
}

impl<T> ApplicationHandler<UserEvent<T>> for WinitHost<T>
where
    T: App + 'static,
{
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent<T>) {
        match event {
            UserEvent::Quit => event_loop.exit(),
            UserEvent::Repaint(token) => {
                if let Some(win) = self.window_by_token(token) {
                    win.next = FramePresent::Immediate;
                }
            }
            UserEvent::RunOnMain(task) => match &mut self.phase {
                HostPhase::Bootstrap(bootstrap) => bootstrap.pending_tasks.push(task),
                HostPhase::Running(runtime) => {
                    if task(&mut runtime.app) {
                        for win in runtime.windows.values_mut() {
                            win.next = FramePresent::Immediate;
                        }
                    }
                }
            },
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let handle = self.handle();
        let HostPhase::Bootstrap(bootstrap) = &mut self.phase else {
            return;
        };
        let runtime = WinitRuntime::new(event_loop, bootstrap, handle);
        self.phase = HostPhase::Running(Box::new(runtime));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Service in-frame window open/close requests before scheduling.
        self.drain_window_requests(event_loop);
        self.sync_diagnostics();

        // Fold every window's `FramePresent` into one `ControlFlow`. A
        // window wanting `Immediate` (or `At(t)` already due) gets its
        // own `request_redraw`; the loop wakes for it regardless of the
        // `WaitUntil`. Future `At(t)`s contribute their deadline; the
        // nearest wins so no window out-sleeps its own schedule.
        let HostPhase::Running(runtime) = &self.phase else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        let now = Instant::now();
        let mut earliest: Option<Instant> = None;
        for win in runtime.windows.values() {
            // `At(t)` with `t <= now` collapses to `Immediate` —
            // `WaitUntil` would fire instantly and loop, so just request
            // the redraw.
            let next = match win.next {
                FramePresent::At(t) if t <= now => FramePresent::Immediate,
                other => other,
            };
            match next {
                FramePresent::Immediate => win.window.request_redraw(),
                FramePresent::At(t) => {
                    earliest = Some(earliest.map_or(t, |e| e.min(t)));
                }
                FramePresent::Idle => {}
            }
        }
        match earliest {
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Drain input into the target window's `Ui` first, in its own
        // scope so the `&mut Window` borrow ends before the match
        // arms re-borrow (`RedrawRequested` needs `&mut self` for
        // `draw`).
        {
            let HostPhase::Running(runtime) = &mut self.phase else {
                return;
            };
            let Some(win) = runtime.windows.get_mut(&id) else {
                return;
            };
            let mut wants_repaint = false;
            input::translate(&event, win.scale_factor, |ev| {
                wants_repaint |= win.on_input(ev).requests_repaint;
            });
            if wants_repaint {
                win.next = FramePresent::Immediate;
            }
        }

        match event {
            WindowEvent::RedrawRequested => self.draw(id),

            WindowEvent::CloseRequested => {
                // Don't remove the window here — flag it and force a frame.
                // `draw` surfaces the flag as `Ui::close_requested` so the
                // app can veto (`Ui::keep_open`) to show a "save changes?"
                // prompt; absent a veto, `draw` closes the window via the
                // normal command path and `drain_window_requests`
                // makes the all-windows-closed exit decision as before.
                if let HostPhase::Running(runtime) = &mut self.phase
                    && let Some(win) = runtime.windows.get_mut(&id)
                {
                    win.close_requested = true;
                    win.next = FramePresent::Immediate;
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let HostPhase::Running(runtime) = &mut self.phase
                    && let Some(win) = runtime.windows.get_mut(&id)
                {
                    win.scale_factor = scale_factor as f32;
                    win.next = FramePresent::Immediate;
                }
            }
            WindowEvent::Resized(new) => {
                if let HostPhase::Running(runtime) = &mut self.phase
                    && let Some(win) = runtime.windows.get_mut(&id)
                {
                    let max = runtime.surfaces.max_texture_dim;
                    let w = new.width.clamp(1, max);
                    let h = new.height.clamp(1, max);
                    // Stash the new size only — `Window::frame`
                    // notices the mismatch against its `configured`
                    // baseline and runs
                    // `surface.configure` once before acquiring the next
                    // swapchain texture, so identical repeats (Wayland
                    // resends configures on focus / output changes) cost
                    // nothing. `surface.configure` waits for GPU idle and
                    // reallocates the swapchain — wgpu #7447 measures
                    // 100ms+ stalls when called per repeated event, which
                    // is what fed the resize backlog.
                    if w != win.config.width || h != win.config.height {
                        win.config.width = w;
                        win.config.height = h;
                        // Defer the paint: inline `self.draw()` in this
                        // handler lags noticeably on this Wayland setup
                        // even with `pre_present_notify` wired up — the
                        // paint blocks on FIFO vsync inside
                        // `surface.get_current_texture` and the compositor
                        // queue drains faster than we drain it. Letting
                        // `about_to_wait` coalesce into one
                        // `RedrawRequested` per loop tick gives the
                        // smoother feel in practice.
                        win.next = FramePresent::Immediate;
                    }
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let HostPhase::Running(runtime) = &mut self.phase
                    && let Some(win) = runtime.windows.get_mut(&id)
                {
                    win.set_occluded(occluded);
                    if !occluded {
                        win.next = FramePresent::Immediate;
                    }
                }
            }

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
