//! `WinitHost` — wraps one-or-more [`WindowRenderer`]s with winit
//! windows, their surfaces, and the [`ApplicationHandler`] event-loop
//! glue. Owns everything below the user's app: a shared GPU context
//! ([`Gpu`]), per-window swapchain config, resize / scale / occlusion
//! handling, and the `FramePresent` scheduling state machine — folded
//! across all windows into one `ControlFlow`.
//!
//! The caller-supplied app implements the [`App`] trait
//! (`frame(&mut self, win: WindowToken, ui: &mut Ui)`, run once per
//! redraw *per window*). The app is built by a closure handed to
//! [`WinitHost::new`], invoked once the first window's `Ui` +
//! [`HostHandle`] are ready (before the first frame) — so startup wiring
//! (theme tweaks, restoring persisted state, stashing the handle) happens
//! there.
//!
//! **Multi-window model.** Every window is an independent UI tree — its
//! own `Ui` (input / focus / layout / `Display`) and [`WindowRenderer`] —
//! all rendering through the one shared
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend) built from one
//! shared [`Gpu`] (`Instance` / `Adapter` / `Device` / `Queue`).
//! Windows are addressed by a caller-chosen [`WindowToken`]; winit's
//! opaque `WindowId` stays internal for event routing. The app opens /
//! closes windows from inside `frame` via [`Ui::open_window`] /
//! [`Ui::close_window`] (see `docs/roadmap/multiwindow.md`).
//!
//! Submodules: [`config`] ([`WinitHostConfig`]), [`handle`]
//! ([`HostHandle`] + [`UserEvent`]), [`gpu`] (the shared wgpu context).
//! The backend-agnostic window vocabulary ([`WindowToken`],
//! [`WindowConfig`]) lives in [`crate::window`].
//!
//! Usage:
//!
//! ```ignore
//! struct MyApp;
//! impl palantir::App for MyApp {
//!     fn frame(&mut self, _win: WindowToken, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::new(WindowToken(0), WinitHostConfig::new("title"), |ui, _handle| {
//!     ui.theme.button.anim = Some(AnimSpec::SPRING);
//!     MyApp
//! })
//! .run();
//! ```

pub(crate) mod config;
pub(crate) mod gpu;
pub(crate) mod handle;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::context::HostContext;
use crate::input::InputEvent;
use crate::renderer::backend::WgpuBackend;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::{PendingWindow, WindowConfig, WindowToken};
use crate::window_renderer::{FramePresent, FrameTarget, PresentStrategy, WindowRenderer};
use crate::winit_host::config::WinitHostConfig;
use crate::winit_host::gpu::{Gpu, GpuInit, WindowSurface};
use crate::winit_host::handle::{HostHandle, MainTask, UserEvent};

/// Builds the caller's app once the first window's `Ui` + [`HostHandle`]
/// exist — handed to [`WinitHost::new`] and invoked on the first
/// `resumed`.
type AppBuilder<T> = Box<dyn FnOnce(&mut Ui, HostHandle<T>) -> T>;

/// The caller-supplied app. `WinitHost` builds it via the closure passed
/// to [`WinitHost::new`] once the first window's `Ui` and [`HostHandle`]
/// exist (after device + surface are up, before the first frame), then
/// calls [`App::frame`] once per redraw, per window — `win` names which.
pub trait App {
    /// Build one frame of window `win`: implementors mutate themselves
    /// and emit widgets. Switch on `win` to drive different windows;
    /// open / close further windows via [`Ui::open_window`] /
    /// [`Ui::close_window`] on `ui`.
    fn frame(&mut self, win: WindowToken, ui: &mut Ui);
}

/// Everything one window owns: its winit handle, swapchain surface +
/// config, the per-window [`WindowRenderer`] (its `Ui` recorder +
/// per-window encode/compose scratch + backbuffer), DPR scale, and the
/// host-side scheduling state. The shared GPU renderer (device/queue,
/// pipelines, atlases) lives on [`Running`], not here.
struct WindowState {
    token: WindowToken,
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
}

/// What [`WinitHost::new`] stashes for the first `resumed`: the bootstrap
/// window's token + config and the caller's app builder. Consumed whole —
/// winit hands out `&ActiveEventLoop` only inside callbacks, so window +
/// GPU + app construction all wait here until then.
struct Bootstrap<T: 'static> {
    token: WindowToken,
    config: WinitHostConfig,
    build: AppBuilder<T>,
}

/// Everything the first `resumed` builds, bundled so "booted" is one
/// `Option` and a half-built state (a backend without an app, …) is
/// unrepresentable.
struct Running<T> {
    /// The caller's app, built by [`Bootstrap::build`] once the first
    /// window's `Ui` existed.
    app: T,
    /// Shared GPU context (instance / adapter / device / queue; surface
    /// factory).
    gpu: Gpu,
    /// Shared, app-global state (render handles + live-window set + debug
    /// overlay) every window's `Ui` clones; each `WindowRenderer` and the
    /// backend (render handles only) derive from it.
    context: HostContext,
    /// The one shared GPU renderer every window draws through (pipelines,
    /// atlases); passed into each window's `WindowRenderer::frame`.
    backend: WgpuBackend,
}

/// Top-level winit-driven palantir runtime. Owns the caller-supplied app
/// `T: App` (RAII lifetime, no `Rc<RefCell<>>` to manage) and calls
/// `T::frame` once per redraw, per window. Two-state lifecycle, one
/// `Option` each: [`Bootstrap`] (pre-`resumed` inputs, consumed once)
/// and [`Running`] (everything the first `resumed` builds).
pub struct WinitHost<T: 'static> {
    /// Deferred-start inputs, consumed by the first `resumed`. `None`
    /// thereafter. The app can't exist before a `Ui` does, so its
    /// construction is necessarily deferred.
    bootstrap: Option<Bootstrap<T>>,
    /// Everything built on the first `resumed`; `None` only before that.
    running: Option<Running<T>>,
    /// `RunOnMain` tasks that arrived before [`Self::running`] existed —
    /// handles are handed out before `run()`, so workers can race
    /// startup. Drained into the app right after the builder runs.
    pending_tasks: Vec<MainTask<T>>,
    /// Live windows, keyed by winit's `WindowId` for event routing.
    windows: HashMap<WindowId, WindowState>,
    event_loop: Option<EventLoop<UserEvent<T>>>,
    proxy: EventLoopProxy<UserEvent<T>>,
}

impl<T> WinitHost<T>
where
    T: App + 'static,
{
    /// `build` constructs the app once the first window's `Ui` +
    /// [`HostHandle`] are ready (after device + surface are up, before
    /// the first frame) — do startup wiring (theme tweaks, restoring
    /// persisted state, stashing the handle) inside it. It runs on the
    /// first `resumed`, not here. `first_token` is the [`WindowToken`]
    /// the bootstrap window is addressed by.
    pub fn new(
        first_token: WindowToken,
        config: WinitHostConfig,
        build: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static,
    ) -> Self {
        // EventLoop is built up front so `handle()` can hand out a proxy
        // before `run()` is called — that's the whole point of letting
        // threads spawn knowing where to send their pokes.
        let event_loop = EventLoop::<UserEvent<T>>::with_user_event()
            .build()
            .expect("event loop");
        let proxy = event_loop.create_proxy();
        Self {
            bootstrap: Some(Bootstrap {
                token: first_token,
                config,
                build: Box::new(build),
            }),
            running: None,
            pending_tasks: Vec::new(),
            windows: HashMap::new(),
            event_loop: Some(event_loop),
            proxy,
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
        self.windows.values_mut().find(|w| w.token == token)
    }

    /// Paint one window. Bundles its surface, config, scale, and monitor
    /// refresh into a [`FrameTarget`], runs the per-window
    /// `WindowRenderer::frame`, and stores the returned schedule back on
    /// the window. The live-window set + debug overlay reach the `Ui`
    /// through the shared host state, not this call.
    fn draw(&mut self, id: WindowId) {
        let Self {
            running, windows, ..
        } = self;
        let (Some(run), Some(win)) = (running.as_mut(), windows.get_mut(&id)) else {
            return;
        };
        let window = win.window.clone();
        let token = win.token;
        // `refresh_millihertz` is queried each frame so a window dragged
        // onto a different-refresh monitor re-paces immediately — winit
        // fires no reliable "refresh changed" event to cache against.
        let refresh_millihertz = win
            .window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz());
        // Surface any pending OS close request to the app for this frame;
        // it may veto (`Ui::keep_open`) to prompt instead of closing.
        win.renderer.ui.wants_close = win.close_requested;
        win.renderer.ui.close_vetoed = false;
        win.next = win.renderer.frame(
            &mut run.backend,
            FrameTarget {
                surface: &win.surface,
                config: &win.config,
                scale_factor: win.scale_factor,
                refresh_millihertz,
            },
            |ui| run.app.frame(token, ui),
            || window.pre_present_notify(),
        );
        // Resolve the close request now the app has had its say. Unless
        // vetoed, route it through the same `pending_closes` path an
        // explicit `Ui::close_window` uses, so `drain_window_requests`
        // handles removal + the all-windows-closed exit uniformly.
        if win.close_requested {
            win.close_requested = false;
            if !win.renderer.ui.close_vetoed {
                win.renderer.ui.pending_closes.push(token);
            }
        }
        win.renderer.ui.wants_close = false;
    }

    /// Build a winit window + surface + `WindowRenderer` for `token` and
    /// insert it into the map. No-ops (with a warning) on a duplicate
    /// token, which the token couldn't then unambiguously address.
    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        token: WindowToken,
        cfg: WindowConfig,
    ) {
        if self.windows.values().any(|w| w.token == token) {
            tracing::warn!(?token, "open_window: token already in use, ignoring");
            return;
        }
        // Open requests only come off live windows' `Ui` queues, which
        // exist only after the first `resumed` booted everything.
        let run = self.running.as_ref().expect("open_window before boot");
        let window = create_window(event_loop, &cfg);
        let ws = run.gpu.make_surface(&window);
        let renderer = WindowRenderer::new(
            &run.context,
            run.gpu.max_texture_dim,
            PresentStrategy::DirectAdaptive,
        );
        self.insert_window(token, window, ws, renderer);
    }

    /// Register a freshly built window in the routing map, scheduled to
    /// paint its first frame (`next: Immediate` makes the next
    /// `about_to_wait` request the redraw). Shared tail of `resumed` and
    /// `spawn_window`.
    fn insert_window(
        &mut self,
        token: WindowToken,
        window: Arc<Window>,
        ws: WindowSurface,
        renderer: WindowRenderer,
    ) {
        let scale_factor = window.scale_factor() as f32;
        let id = window.id();
        self.windows.insert(
            id,
            WindowState {
                token,
                window,
                surface: ws.surface,
                config: ws.config,
                renderer,
                scale_factor,
                next: FramePresent::Immediate,
                close_requested: false,
            },
        );
    }

    /// Drain every window's [`Ui::open_window`] / [`Ui::close_window`]
    /// queues and apply them. Runs in `about_to_wait`, the one callback
    /// that always holds `&ActiveEventLoop` after event processing.
    /// Requests are collected out of the per-window queues *first* so the
    /// subsequent `create_window` inserts don't alias the map we're
    /// iterating.
    fn drain_window_requests(&mut self, event_loop: &ActiveEventLoop) {
        let mut opens: Vec<PendingWindow> = Vec::new();
        let mut closes: Vec<WindowToken> = Vec::new();
        for win in self.windows.values_mut() {
            opens.append(&mut win.renderer.ui.pending_windows);
            closes.append(&mut win.renderer.ui.pending_closes);
        }
        // Closes first, so a same-frame close + open of one token
        // recreates the window instead of tripping `spawn_window`'s
        // duplicate-token guard and losing it.
        for token in closes {
            self.windows.retain(|_, win| win.token != token);
        }
        for pw in opens {
            self.spawn_window(event_loop, pw.token, pw.config);
        }
        if self.windows.is_empty() && self.running.is_some() {
            // Every window closed (titlebar X or `close_window`) — nothing
            // left to drive.
            event_loop.exit();
        }
    }

    /// Reconcile the shared host state with the live window set after a
    /// drain: publish the current tokens for `Ui::window_open`, and if a
    /// window toggled the app-global debug overlay
    /// (`Ui::debug_overlay_mut`), force every window to repaint so the
    /// change shows on idle ones — they're otherwise damage-`Skip` and
    /// would never pick it up. Runs in `about_to_wait`.
    fn sync_host_state(&mut self) {
        let Self {
            running, windows, ..
        } = self;
        let Some(run) = running.as_mut() else {
            return;
        };
        run.context
            .set_open_windows(windows.values().map(|w| w.token));
        if run.context.take_overlay_dirty() {
            for win in windows.values_mut() {
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
    let mut attrs = Window::default_attributes().with_title(cfg.title.clone());
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.x, s.y));
    }
    Arc::new(event_loop.create_window(attrs).expect("create window"))
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
            UserEvent::RunOnMain(task) => {
                // The task folds background-thread results into app state
                // (`&mut T`). A `true` return repaints every window, since
                // any of them may read the changed state next frame.
                let Some(run) = self.running.as_mut() else {
                    // Raced startup (handles exist before `run()`); held
                    // until `resumed` builds the app, never dropped.
                    self.pending_tasks.push(task);
                    return;
                };
                if task(&mut run.app) {
                    for win in self.windows.values_mut() {
                        win.next = FramePresent::Immediate;
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Only the first `resumed` acts; the bootstrap is gone on a
        // post-suspend resume and desktop targets keep their surfaces.
        let Some(boot) = self.bootstrap.take() else {
            return;
        };

        let window = create_window(event_loop, &boot.config.window);
        let GpuInit {
            gpu,
            first_surface: ws,
        } = Gpu::create(&window, &boot.config);
        // Shared resources first, then the one shared GPU renderer built
        // from them; every window's `Ui` + the backend derive from `ctx`
        // (which also carries the app-global window/overlay state).
        let ctx = HostContext::new(TextShaper::with_bundled_fonts());
        let backend = gpu.make_backend(&ctx);
        let mut renderer =
            WindowRenderer::new(&ctx, gpu.max_texture_dim, PresentStrategy::DirectAdaptive);

        // Build the app now that the first `Ui` exists.
        let mut app = (boot.build)(&mut renderer.ui, self.handle());
        // `RunOnMain` tasks that raced startup. Their repaint returns are
        // moot — every window paints its first frame `Immediate` anyway.
        for task in self.pending_tasks.drain(..) {
            task(&mut app);
        }

        self.insert_window(boot.token, window, ws, renderer);
        self.running = Some(Running {
            app,
            gpu,
            context: ctx,
            backend,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Service in-frame window open/close requests before scheduling.
        self.drain_window_requests(event_loop);
        // Republish the live-window set + broadcast any debug-overlay
        // toggle to the shared host state before scheduling redraws.
        self.sync_host_state();

        // Fold every window's `FramePresent` into one `ControlFlow`. A
        // window wanting `Immediate` (or `At(t)` already due) gets its
        // own `request_redraw`; the loop wakes for it regardless of the
        // `WaitUntil`. Future `At(t)`s contribute their deadline; the
        // nearest wins so no window out-sleeps its own schedule.
        let now = Instant::now();
        let mut earliest: Option<Instant> = None;
        for win in self.windows.values() {
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
            let Some(win) = self.windows.get_mut(&id) else {
                return;
            };
            let mut wants_repaint = false;
            InputEvent::from_winit(&event, win.scale_factor, |ev| {
                wants_repaint |= win.renderer.ui.on_input(ev).requests_repaint;
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
                // normal `pending_closes` path and `drain_window_requests`
                // makes the all-windows-closed exit decision as before.
                if let Some(win) = self.windows.get_mut(&id) {
                    win.close_requested = true;
                    win.next = FramePresent::Immediate;
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.scale_factor = scale_factor as f32;
                    win.next = FramePresent::Immediate;
                }
            }
            WindowEvent::Resized(new) => {
                // A window event only fires after `resumed` booted, so
                // `running` is always present here.
                let max = self
                    .running
                    .as_ref()
                    .expect("booted before any window event")
                    .gpu
                    .max_texture_dim;
                if let Some(win) = self.windows.get_mut(&id) {
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
                if let Some(win) = self.windows.get_mut(&id) {
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
