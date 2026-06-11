//! `WinitHost` — wraps one-or-more [`Host`]s with winit windows, their
//! surfaces, and the [`ApplicationHandler`] event-loop glue. Owns
//! everything below the user's app: a shared GPU context ([`Gpu`]),
//! per-window swapchain config, resize / scale / occlusion handling,
//! and the `FramePresent` scheduling state machine — folded across all
//! windows into one `ControlFlow`.
//!
//! The caller-supplied app implements the [`App`] trait
//! (`frame(&mut self, win: WindowToken, ui: &mut Ui)`, run once per
//! redraw *per window*). The app is built by a closure handed to
//! [`WinitHost::new`], invoked once the first window's `Ui` +
//! [`HostHandle`] are ready (before the first frame) — so startup wiring
//! (theme tweaks, restoring persisted state, stashing the handle) happens
//! there.
//!
//! **Multi-window model.** Every window is an independent UI tree: its
//! own `Ui` (input / focus / layout / `Display`) and its own
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend), all built from
//! one shared [`Gpu`] (`Instance` / `Adapter` / `Device` / `Queue`).
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

use glam::UVec2;

use crate::Display;
use crate::host::{FramePresent, Host};
use crate::input::InputEvent;
use crate::ui::Ui;
use crate::window::{PendingWindow, WindowConfig, WindowToken};
use crate::winit_host::config::WinitHostConfig;
use crate::winit_host::gpu::{Gpu, GpuInit, WindowSurface};
use crate::winit_host::handle::{HostHandle, UserEvent};

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
/// config, the per-window [`Host`] (recorder + renderer), DPR scale, and
/// the host-side scheduling state. The GPU device/queue live on the
/// shared [`Gpu`], not here.
struct WindowState {
    token: WindowToken,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    host: Host,
    scale_factor: f32,
    /// Host-side scheduling state. Reset at the top of `draw` from the
    /// `FramePresent` the frame returned; re-armed to `Immediate` by
    /// input, resize, surface loss, occlusion, and animation tickers.
    next: FramePresent,
}

/// Top-level winit-driven palantir runtime. Owns the caller-supplied app
/// `T: App` (RAII lifetime, no `Rc<RefCell<>>` to manage) and calls
/// `T::frame` once per redraw, per window.
pub struct WinitHost<T: 'static> {
    /// Config for the first window, consumed by the first `resumed`.
    config: WinitHostConfig,
    /// Token assigned to the first (bootstrap) window.
    first_token: WindowToken,
    /// `None` until `resumed` builds the first `Ui` and runs the app
    /// builder; the app lands here. The app can't exist before a `Ui`
    /// does, so construction is necessarily deferred.
    app: Option<T>,
    /// The caller's app builder, set by [`WinitHost::new`] and consumed
    /// by the first `resumed`. `None` after the build.
    app_builder: Option<AppBuilder<T>>,
    /// Shared GPU context, built lazily on the first `resumed`.
    gpu: Option<Gpu>,
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
            config,
            first_token,
            app: None,
            app_builder: Some(Box::new(build)),
            gpu: None,
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

    /// Paint one window. Reads its surface size + monitor refresh into a
    /// fresh `Display`, runs the per-window `Host::frame`, and stores the
    /// returned schedule back on the window.
    fn draw(&mut self, id: WindowId) {
        let Self { app, windows, .. } = self;
        let (Some(app), Some(rt)) = (app.as_mut(), windows.get_mut(&id)) else {
            return;
        };
        let window = rt.window.clone();
        let token = rt.token;
        // `refresh_millihertz` is queried each frame so a window dragged
        // onto a different-refresh monitor re-paces immediately — winit
        // fires no reliable "refresh changed" event to cache against.
        let display = Display {
            refresh_millihertz: rt
                .window
                .current_monitor()
                .and_then(|m| m.refresh_rate_millihertz()),
            ..Display::from_physical(
                UVec2::new(rt.config.width, rt.config.height),
                rt.scale_factor,
            )
        };
        rt.next = rt.host.frame(
            &rt.surface,
            &rt.config,
            display,
            |ui| app.frame(token, ui),
            || window.pre_present_notify(),
        );
    }

    /// Build a winit window + surface + `Host` for `token` and insert it
    /// into the map. No-ops (with a warning) on a duplicate token, which
    /// the token couldn't then unambiguously address.
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
        let window = create_window(event_loop, &cfg);
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let ws = gpu.make_surface(&window);
        let host = gpu.make_host(ws.config.format);
        self.insert_window(token, window, ws, host);
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
        host: Host,
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
                host,
                scale_factor,
                next: FramePresent::Immediate,
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
        for rt in self.windows.values_mut() {
            opens.append(&mut rt.host.ui.pending_windows);
            closes.append(&mut rt.host.ui.pending_closes);
        }
        for pw in opens {
            self.spawn_window(event_loop, pw.token, pw.config);
        }
        for token in closes {
            self.windows.retain(|_, rt| rt.token != token);
        }
        if self.windows.is_empty() && self.gpu.is_some() {
            // Every window closed (titlebar X or `close_window`) — nothing
            // left to drive.
            event_loop.exit();
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
                if let Some(rt) = self.window_by_token(token) {
                    rt.next = FramePresent::Immediate;
                }
            }
            UserEvent::RunOnMain(task) => {
                // The task folds background-thread results into app state
                // (`&mut T`). A `true` return repaints every window, since
                // any of them may read the changed state next frame.
                let repaint = match self.app.as_mut() {
                    Some(app) => task(app),
                    None => false,
                };
                if repaint {
                    for rt in self.windows.values_mut() {
                        rt.next = FramePresent::Immediate;
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }

        let cfg = self.config.clone();
        let window = create_window(event_loop, &cfg.window);
        let GpuInit {
            gpu,
            first_surface: ws,
        } = Gpu::create(&window, &cfg);
        let mut host = gpu.make_host(ws.config.format);

        // Build the app now that the first `Ui` exists. The
        // `gpu.is_some()` early-return above means this runs exactly once
        // (a post-suspend `resumed` returns there), so no `app.is_none()`
        // guard is needed; `app_builder.take().expect(..)` is the
        // double-build backstop.
        let handle = HostHandle {
            proxy: self.proxy.clone(),
        };
        let build = self.app_builder.take().expect("app builder consumed");
        self.app = Some(build(&mut host.ui, handle));

        self.insert_window(self.first_token, window, ws, host);
        self.gpu = Some(gpu);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Service in-frame window open/close requests before scheduling.
        self.drain_window_requests(event_loop);

        // Fold every window's `FramePresent` into one `ControlFlow`. A
        // window wanting `Immediate` (or `At(t)` already due) gets its
        // own `request_redraw`; the loop wakes for it regardless of the
        // `WaitUntil`. Future `At(t)`s contribute their deadline; the
        // nearest wins so no window out-sleeps its own schedule.
        let now = Instant::now();
        let mut earliest: Option<Instant> = None;
        for rt in self.windows.values() {
            // `At(t)` with `t <= now` collapses to `Immediate` —
            // `WaitUntil` would fire instantly and loop, so just request
            // the redraw.
            let next = match rt.next {
                FramePresent::At(t) if t <= now => FramePresent::Immediate,
                other => other,
            };
            match next {
                FramePresent::Immediate => rt.window.request_redraw(),
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Drain input into the target window's `Ui` first, in its own
        // scope so the `&mut WindowState` borrow ends before the match
        // arms re-borrow (`RedrawRequested` needs `&mut self` for
        // `draw`).
        {
            let Some(rt) = self.windows.get_mut(&id) else {
                return;
            };
            let mut wants_repaint = false;
            InputEvent::from_winit(&event, rt.scale_factor, |ev| {
                wants_repaint |= rt.host.ui.on_input(ev).requests_repaint;
            });
            if wants_repaint {
                rt.next = FramePresent::Immediate;
            }
        }

        match event {
            WindowEvent::RedrawRequested => self.draw(id),

            WindowEvent::CloseRequested => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(rt) = self.windows.get_mut(&id) {
                    rt.scale_factor = scale_factor as f32;
                    rt.next = FramePresent::Immediate;
                }
            }
            WindowEvent::Resized(new) => {
                // A window event only fires after `resumed` built the GPU,
                // so the shared context is always present here.
                let max = self
                    .gpu
                    .as_ref()
                    .expect("gpu built before any window event")
                    .device
                    .limits()
                    .max_texture_dimension_2d;
                if let Some(rt) = self.windows.get_mut(&id) {
                    let w = new.width.clamp(1, max);
                    let h = new.height.clamp(1, max);
                    // Stash the new size only — `Host::frame` notices the
                    // mismatch against its `configured` baseline and runs
                    // `surface.configure` once before acquiring the next
                    // swapchain texture, so identical repeats (Wayland
                    // resends configures on focus / output changes) cost
                    // nothing. `surface.configure` waits for GPU idle and
                    // reallocates the swapchain — wgpu #7447 measures
                    // 100ms+ stalls when called per repeated event, which
                    // is what fed the resize backlog.
                    if w != rt.config.width || h != rt.config.height {
                        rt.config.width = w;
                        rt.config.height = h;
                        // Defer the paint: inline `self.draw()` in this
                        // handler lags noticeably on this Wayland setup
                        // even with `pre_present_notify` wired up — the
                        // paint blocks on FIFO vsync inside
                        // `surface.get_current_texture` and the compositor
                        // queue drains faster than we drain it. Letting
                        // `about_to_wait` coalesce into one
                        // `RedrawRequested` per loop tick gives the
                        // smoother feel in practice.
                        rt.next = FramePresent::Immediate;
                    }
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(rt) = self.windows.get_mut(&id) {
                    rt.host.set_occluded(occluded);
                    if !occluded {
                        rt.next = FramePresent::Immediate;
                    }
                }
            }

            _ => {}
        }
    }
}
