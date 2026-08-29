//! `WinitHost` — the winit [`ApplicationHandler`] glue around a
//! [`WinitRuntime`]. Its lifecycle is encoded by [`HostPhase`]: bootstrap
//! inputs become one runtime containing the app, shared render core, surface
//! manager, and complete live-window set; a failure short-circuits the
//! callback-driven loop so [`WinitHost::run`] can return it.
//!
//! This file owns only what winit's *lifecycle* dictates — deferred
//! construction (winit hands out `&ActiveEventLoop` only inside callbacks) and
//! event dispatch. Everything a windowed host would do regardless of winit
//! lives in [`runtime`]; the winit *types* are converted in [`native`] and
//! [`input`].
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
//! `Ui` (input / focus / layout / `Display`) and
//! [`WindowDriver`](crate::host::window_driver::WindowDriver) — all rendering
//! serially through the one shared [`HostCore`](crate::host::core::HostCore).
//! Windows are addressed by a caller-chosen [`WindowToken`]; winit's opaque
//! `WindowId` stays internal for event routing. The app opens / closes windows
//! from inside `record` via [`Ui::open_window`] / [`Ui::close_window`].
//!
//! Submodules: [`config`] ([`WinitHostConfig`]), [`error`]
//! ([`WinitHostError`]), [`handle`] ([`HostHandle`] + [`UserEvent`]), [`gpu`]
//! (surface/device startup), [`native`] (winit type conversion + window
//! creation), [`runtime`] ([`WinitRuntime`]), and [`window`] (per-window
//! swapchain frames). The backend-agnostic window vocabulary
//! ([`WindowToken`], [`WindowConfig`](crate::window::window_config::WindowConfig)) lives in
//! [`crate::window`].
//!
//! Usage:
//!
//! ```no_run
//! # use palantir::{AnimSpec, Theme, Ui, WindowToken, WinitHost, WinitHostError};
//! # fn demo() -> Result<(), WinitHostError> {
//! struct MyApp;
//! impl palantir::App for MyApp {
//!     fn record(&mut self, _win: WindowToken, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::builder(WindowToken(0))
//!     .title("title")
//!     .build(|ui, _handle| {
//!         let mut theme = Theme::default();
//!         theme.button.anim = Some(AnimSpec::SPRING);
//!         ui.set_theme(theme);
//!         MyApp
//!     })?
//!     .run()?;
//! # Ok(())
//! # }
//! ```

pub(crate) mod config;
pub(crate) mod error;
mod gpu;
pub(crate) mod handle;
mod input;
mod native;
mod runtime;
mod window;
mod window_set;

use std::marker::PhantomData;
use std::time::Instant;

use glam::UVec2;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::app::App;
use crate::display;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::gpu::SurfaceManager;
use crate::host::winit::handle::{HostHandle, MainTask, UserEvent};
use crate::host::winit::runtime::WinitRuntime;
use crate::host::winit::window::FramePresent;
use crate::ui::Ui;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

type AppFactory<T> = Box<dyn FnOnce(&mut Ui, HostHandle<T>) -> T>;

/// What [`WinitHostBuilder::build`] stashes for the first `resumed`: the
/// bootstrap window's token + config and the caller's app factory. Consumed —
/// winit hands out `&ActiveEventLoop` only inside callbacks, so window +
/// GPU + app construction all wait here until then.
pub(super) struct Bootstrap<T: 'static> {
    pub(super) token: WindowToken,
    pub(super) config: WinitHostConfig,
    pub(super) create_app: Option<AppFactory<T>>,
    pub(super) pending_tasks: Vec<MainTask<T>>,
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

enum HostPhase<T: 'static> {
    Bootstrap(Bootstrap<T>),
    Running(Box<WinitRuntime<T>>),
    Failed(WinitHostError),
}

impl<T: 'static> std::fmt::Debug for HostPhase<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bootstrap(bootstrap) => f.debug_tuple("Bootstrap").field(bootstrap).finish(),
            Self::Running(runtime) => f.debug_tuple("Running").field(runtime).finish(),
            Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
        }
    }
}

/// Top-level winit-driven palantir runtime. Owns the caller-supplied app
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
    ///
    /// Naming a [`wgpu::PresentMode`] means depending on wgpu directly, at a
    /// version matching the one palantir links. [`Self::vsync`] covers the
    /// common case without that.
    pub fn present_mode(mut self, mode: wgpu::PresentMode) -> Self {
        self.config.present_mode = mode;
        self
    }

    /// Start every window with `vsync` — the launch-time twin of
    /// [`Ui::set_vsync`](crate::Ui::set_vsync), in the same backend-neutral
    /// vocabulary.
    ///
    /// Prefer this over asking for the same thing from the first frame: set
    /// here it reaches the *initial* swapchain, where the runtime request
    /// would build one swapchain and immediately replace it.
    pub fn vsync(mut self, vsync: Vsync) -> Self {
        self.config.present_mode = gpu::present_mode(vsync);
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
    ///
    /// # Errors
    ///
    /// Returns an error when winit cannot create the event loop.
    pub fn build(
        self,
        create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static,
    ) -> Result<WinitHost<T>, WinitHostError> {
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
        let event_loop = event_loop_builder
            .build()
            .map_err(|source| WinitHostError::CreateEventLoop { source })?;
        let proxy = event_loop.create_proxy();
        Ok(WinitHost {
            phase: HostPhase::Bootstrap(Bootstrap {
                token: self.first_token,
                config: self.config,
                create_app: Some(Box::new(create_app)),
                pending_tasks: Vec::new(),
            }),
            event_loop: Some(event_loop),
            proxy,
        })
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

    /// Drive the already-constructed event loop to completion.
    ///
    /// # Errors
    ///
    /// Returns event-loop failures and any window, surface, adapter, or device
    /// failure encountered during deferred startup or secondary-window creation.
    pub fn run(mut self) -> Result<(), WinitHostError> {
        let event_loop = self.event_loop.take().expect("event loop already consumed");
        let event_loop_result = event_loop.run_app(&mut self);
        let failure = match self.phase {
            HostPhase::Failed(error) => Some(error),
            HostPhase::Bootstrap(_) | HostPhase::Running(_) => None,
        };
        finish_run(failure, event_loop_result)
    }

    /// The live runtime, or `None` before `resumed` and after a failure.
    fn running(&mut self) -> Option<&mut WinitRuntime<T>> {
        match &mut self.phase {
            HostPhase::Running(runtime) => Some(runtime),
            HostPhase::Bootstrap(_) | HostPhase::Failed(_) => None,
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: WinitHostError) {
        self.phase = HostPhase::Failed(error);
        event_loop.exit();
    }
}

fn finish_run(
    failure: Option<WinitHostError>,
    event_loop_result: Result<(), winit::error::EventLoopError>,
) -> Result<(), WinitHostError> {
    match failure {
        Some(error) => Err(error),
        None => event_loop_result.map_err(|source| WinitHostError::RunEventLoop { source }),
    }
}

impl<T> ApplicationHandler<UserEvent<T>> for WinitHost<T>
where
    T: App + 'static,
{
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent<T>) {
        match event {
            UserEvent::Quit => event_loop.exit(),
            UserEvent::Repaint(token) => {
                if let Some(runtime) = self.running()
                    && let Some(win) = runtime.by_token(token)
                {
                    win.next = FramePresent::Immediate;
                }
            }
            UserEvent::RunOnMain(task) => match &mut self.phase {
                HostPhase::Bootstrap(bootstrap) => bootstrap.pending_tasks.push(task),
                HostPhase::Running(runtime) => {
                    if task(&mut runtime.app) {
                        runtime.repaint_all();
                    }
                }
                HostPhase::Failed(_) => {}
            },
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let handle = self.handle();
        let HostPhase::Bootstrap(bootstrap) = &mut self.phase else {
            return;
        };
        match WinitRuntime::new(event_loop, bootstrap, handle) {
            Ok(runtime) => self.phase = HostPhase::Running(Box::new(runtime)),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let Some(runtime) = self.running() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        // Service in-frame window open/close requests before scheduling.
        if let Err(error) = runtime.drain_window_requests(event_loop) {
            self.fail(event_loop, error);
            return;
        }
        runtime.repaint_on_overlay_change();
        runtime.schedule(event_loop, now);
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(runtime) = self.running() else {
            return;
        };
        let max_texture_dim = runtime.surfaces.max_texture_dim;
        // Resolved once for the whole event: the dispatch below names the
        // window by its slot, so a redraw does not look it up again.
        let Some(slot) = runtime.slot_of_id(id) else {
            return;
        };
        let win = runtime.window(slot);

        let mut wants_repaint = false;
        input::translate(&event, win.scale_factor, |ev| {
            wants_repaint |= win.on_input(ev).requests_repaint;
        });
        if wants_repaint {
            win.next = FramePresent::Immediate;
        }

        match event {
            WindowEvent::RedrawRequested => runtime.draw(slot),

            WindowEvent::CloseRequested => {
                // Don't remove the window here — flag it and force a frame.
                // `Window::frame` surfaces the flag as `Ui::close_requested`
                // so the app can veto (`Ui::keep_open`) to show a "save
                // changes?" prompt; absent a veto the frame emits the close
                // through the normal command path and
                // `drain_window_requests` makes the all-windows-closed exit
                // decision as before.
                win.close_requested = true;
                win.next = FramePresent::Immediate;
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                win.scale_factor = display::sanitize_scale_factor(scale_factor);
                win.invalidate_system_facts();
                win.next = FramePresent::Immediate;
            }
            // Nothing else to do with a move: the position is a fact the
            // app asks for, and the monitor under the window is what the
            // driver paces by. Both are cached — see `WindowFacts`.
            WindowEvent::Moved(_) => win.invalidate_system_facts(),
            WindowEvent::Resized(new) => {
                win.invalidate_system_facts();
                let size = SurfaceManager::clamp_extent(
                    max_texture_dim,
                    UVec2::new(new.width, new.height),
                );
                // Stash the new size only — `Window::frame` notices the
                // mismatch against its noted target key and reconfigures the
                // surface once before acquiring the next swapchain texture.
                //
                // Defer the paint: inline drawing in this handler lags
                // noticeably on Wayland even with `pre_present_notify` wired
                // up — the paint blocks on FIFO vsync inside
                // `surface.get_current_texture` and the compositor queue
                // drains faster than we drain it. Letting `about_to_wait`
                // coalesce into one `RedrawRequested` per loop tick gives the
                // smoother feel in practice.
                if size.x != win.config.width || size.y != win.config.height {
                    win.config.width = size.x;
                    win.config.height = size.y;
                    win.next = FramePresent::Immediate;
                }
            }
            WindowEvent::Occluded(occluded) => {
                win.set_occluded(occluded);
                if !occluded {
                    // A window can be moved or re-parented while it is
                    // hidden, and no `Moved` arrives for one that is.
                    win.invalidate_system_facts();
                    win.next = FramePresent::Immediate;
                }
            }

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
