//! `WinitHost` — wraps one-or-more [`WindowRenderer`]s with winit
//! windows, their surfaces, and the [`ApplicationHandler`] event-loop
//! glue. Its lifecycle is encoded by [`HostPhase`]: bootstrap inputs become one
//! [`WinitRuntime`] containing the app, shared-resource root, surface factory,
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
//! **Multi-window model.** Every window is an independent UI tree — its
//! own `Ui` (input / focus / layout / `Display`) and [`WindowRenderer`] —
//! all rendering through one shared
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The backend solely
//! owns the device and queue; [`SurfaceFactory`] retains only the instance,
//! adapter, presentation policy, and texture limit needed by later windows.
//! Windows are addressed by a caller-chosen [`WindowToken`]; winit's
//! opaque `WindowId` stays internal for event routing. The app opens /
//! closes windows from inside `record` via [`Ui::open_window`] /
//! [`Ui::close_window`].
//!
//! Submodules: [`config`] ([`WinitHostConfig`]), [`handle`]
//! ([`HostHandle`] + [`UserEvent`]), and [`gpu`] (surface/backend startup).
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

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use glam::IVec2;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Icon, Window, WindowId};

use crate::app::App;
use crate::host::shared::HostShared;
use crate::host::window_renderer::{FramePresent, WindowFrameInput, WindowRenderer};
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::gpu::{GpuInit, SurfaceFactory, WindowSurface};
use crate::host::winit::handle::{HostHandle, MainTask, UserEvent};
use crate::input::InputEvent;
use crate::renderer::backend::WgpuBackend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::{CursorIcon, WindowCommands, WindowConfig, WindowFrameState, WindowToken};

type AppFactory<T> = Box<dyn FnOnce(&mut Ui, HostHandle<T>) -> T>;

/// Everything one window owns: its winit handle, swapchain surface +
/// config, the per-window [`WindowRenderer`] (its `Ui` recorder +
/// per-window encode/compose scratch + backbuffer), DPR scale, and the
/// host-side scheduling state. The shared GPU renderer (device/queue,
/// pipelines, atlases) lives on `WinitRuntime`, not here.
#[derive(Debug)]
struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: WindowRenderer,
    scale_factor: f32,
    /// Per-window scheduling state. Reset at the top of `draw` from the
    /// `FramePresent` the frame returned; re-armed to `Immediate` by
    /// input, resize, surface loss, occlusion, and animation tickers.
    next: FramePresent,
    /// Set when the OS delivers `WindowEvent::CloseRequested` (titlebar X),
    /// cleared once the next `draw` resolves it. `draw` mirrors it into the
    /// window's `Ui` (`Ui::close_requested`) for that frame and, unless the
    /// app vetoed via `Ui::keep_open`, closes the window afterward. This
    /// deferral is what lets an app show a "save changes?" prompt instead
    /// of the window vanishing on the click.
    close_requested: bool,
    /// The cursor last applied to the OS window, so `draw` only calls
    /// `Window::set_cursor` when the frame's request actually changed.
    cursor: CursorIcon,
}

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
    /// Retained surface-creation state; device and queue live on the backend.
    surfaces: SurfaceFactory,
    /// Shared, app-global state (render handles + live-window set + debug
    /// overlay) every window's `Ui` clones; each `WindowRenderer` and the
    /// backend (render handles only) derive from it.
    shared: HostShared,
    /// The one shared GPU renderer every window draws through (pipelines,
    /// atlases); passed into each window's `WindowRenderer::frame`.
    backend: WgpuBackend,
    windows: HashMap<WindowId, WindowState>,
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

    /// Set the swapchain present mode for every window's surface.
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
    fn window_by_token(&mut self, token: WindowToken) -> Option<&mut WindowState> {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return None;
        };
        runtime
            .windows
            .values_mut()
            .find(|w| w.renderer.token == token)
    }

    /// Paint one window. Bundles its surface, config, scale, monitor refresh,
    /// and window state into a [`WindowFrameInput`], runs the per-window
    /// `WindowRenderer::frame`, and stores the returned schedule back on the
    /// window. The live-window set + debug overlay reach the `Ui` through the
    /// shared host state, not this call.
    fn draw(&mut self, id: WindowId) {
        let HostPhase::Running(runtime) = &mut self.phase else {
            return;
        };
        let Some(win) = runtime.windows.get_mut(&id) else {
            return;
        };
        let window = win.window.clone();
        // `refresh_millihertz` is queried each frame so a window dragged
        // onto a different-refresh monitor re-paces immediately — winit
        // fires no reliable "refresh changed" event to cache against.
        let refresh_millihertz = win
            .window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz());
        let position = win
            .window
            .outer_position()
            .ok()
            .map(|p| IVec2::new(p.x, p.y));
        let mut output = win.renderer.frame(
            &mut runtime.backend,
            WindowFrameInput {
                surface: &win.surface,
                config: &win.config,
                scale_factor: win.scale_factor,
                refresh_millihertz,
                state: WindowFrameState {
                    close_requested: win.close_requested,
                    position,
                    maximized: win.window.is_maximized(),
                },
            },
            &mut runtime.app,
            || window.pre_present_notify(),
        );
        win.next = output.present;
        if output.cursor != win.cursor {
            win.window.set_cursor(winit_cursor(output.cursor));
            win.cursor = output.cursor;
        }
        win.close_requested = false;
        runtime.pending_commands.append(&mut output.commands);
    }

    /// Build a winit window + surface + `WindowRenderer` for `token` and
    /// insert it into the map. No-ops (with a warning) on a duplicate
    /// token, which the token couldn't then unambiguously address.
    fn spawn_window(
        runtime: &mut WinitRuntime<T>,
        event_loop: &ActiveEventLoop,
        token: WindowToken,
        cfg: WindowConfig,
    ) {
        if runtime.windows.values().any(|w| w.renderer.token == token) {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return;
        }
        let window = create_window(event_loop, &cfg);
        let ws = runtime.surfaces.make_surface(&window);
        let renderer =
            WindowRenderer::builder(token, &runtime.shared, runtime.surfaces.max_texture_dim)
                .build();
        Self::insert_window(runtime, window, ws, renderer);
    }

    /// Register a freshly built window in the routing map, scheduled to
    /// paint its first frame (`next: Immediate` makes the next
    /// `about_to_wait` request the redraw). Shared tail of `resumed` and
    /// `spawn_window`.
    fn insert_window(
        runtime: &mut WinitRuntime<T>,
        window: Arc<Window>,
        ws: WindowSurface,
        renderer: WindowRenderer,
    ) {
        runtime.shared.windows.insert(renderer.token);
        Self::insert_window_state(&mut runtime.windows, window, ws, renderer);
    }

    fn insert_window_state(
        windows: &mut HashMap<WindowId, WindowState>,
        window: Arc<Window>,
        ws: WindowSurface,
        renderer: WindowRenderer,
    ) {
        let scale_factor = window.scale_factor() as f32;
        let id = window.id();
        let previous = windows.insert(
            id,
            WindowState {
                window,
                surface: ws.surface,
                config: ws.config,
                renderer,
                scale_factor,
                next: FramePresent::Immediate,
                close_requested: false,
                cursor: CursorIcon::default(),
            },
        );
        assert!(previous.is_none(), "winit returned a duplicate WindowId");
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
        commands.append(&mut runtime.pending_commands);
        // Closes first, so a same-frame close + open of one token
        // recreates the window instead of tripping `spawn_window`'s
        // duplicate-token guard and losing it.
        for token in commands.closes {
            if runtime
                .windows
                .values()
                .any(|win| win.renderer.token == token)
            {
                runtime.windows.retain(|_, win| win.renderer.token != token);
                runtime.shared.windows.remove(token);
            }
        }
        for pw in commands.opens {
            Self::spawn_window(runtime, event_loop, pw.token, pw.config);
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
        if runtime.shared.diagnostics.take_overlay_dirty() {
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
fn create_window(event_loop: &ActiveEventLoop, cfg: &WindowConfig) -> Arc<Window> {
    let mut attrs = Window::default_attributes()
        .with_title(cfg.title.clone())
        .with_maximized(cfg.maximized);
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.x, s.y));
    }
    // Title-bar / taskbar icon (X11/Wayland/Windows; macOS ignores it and
    // uses the .app bundle icon instead). A malformed buffer just yields no
    // icon rather than aborting window creation.
    if let Some(ic) = &cfg.icon
        && let Ok(icon) = Icon::from_rgba(ic.rgba.clone(), ic.width, ic.height)
    {
        attrs = attrs.with_window_icon(Some(icon));
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
        let (token, config, create_app, mut pending_tasks) = match &mut self.phase {
            HostPhase::Bootstrap(bootstrap) => (
                bootstrap.token,
                bootstrap.config.clone(),
                bootstrap
                    .create_app
                    .take()
                    .expect("bootstrap app factory already consumed"),
                std::mem::take(&mut bootstrap.pending_tasks),
            ),
            HostPhase::Running(_) => return,
        };

        let window = create_window(event_loop, &config.window);
        let shared = HostShared::new(TextShaper::with_bundled_fonts());
        let GpuInit {
            surfaces,
            backend,
            first_surface: ws,
        } = GpuInit::new(&window, &config, &shared);
        let mut renderer =
            WindowRenderer::builder(token, &shared, surfaces.max_texture_dim).build();

        shared.windows.insert(token);
        let mut app = create_app(&mut renderer.ui, self.handle());
        for task in pending_tasks.drain(..) {
            task(&mut app);
        }

        let mut runtime = WinitRuntime {
            app,
            surfaces,
            shared,
            backend,
            windows: HashMap::new(),
            pending_commands: WindowCommands::default(),
        };
        Self::insert_window_state(&mut runtime.windows, window, ws, renderer);
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
        // scope so the `&mut WindowState` borrow ends before the match
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
            InputEvent::from_winit(&event, win.scale_factor, |ev| {
                wants_repaint |= win.renderer.on_input(ev).requests_repaint;
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
                    // Stash the new size only — `WindowRenderer::frame`
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
                    win.renderer.set_occluded(occluded);
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
