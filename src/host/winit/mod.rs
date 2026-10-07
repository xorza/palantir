//! `WinitHost` — the winit [`ApplicationHandler`] glue around a [`WinitRuntime`], with its lifecycle encoded by [`HostPhase`].
//!
//! This file owns only what winit's lifecycle dictates: deferred construction (winit hands out `&ActiveEventLoop` only inside callbacks) and event dispatch. Shared host work lives in [`runtime`]; winit types are converted in [`native`] and [`input`].
//!
//! The app implements [`App`]: [`App::update`] runs once before a fully recorded frame, [`App::record`] may replay for warmup or relayout. The closure given to [`WinitHostBuilder::build`] makes the app once the first window's `Ui` and [`HostHandle`] are ready.
//!
//! Usage:
//!
//! Usage:
//!
//! ```no_run
//! # use palantir::{AnimationSpec, Theme, Ui, WindowToken, WinitHost, WinitHostError};
//! # fn demo() -> Result<(), WinitHostError> {
//! struct MyApp;
//! impl palantir::App for MyApp {
//!     fn record(&mut self, _window: WindowToken, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::builder(WindowToken(0))
//!     .title("title")
//!     .build(|ui, _handle| {
//!         let mut theme = Theme::default();
//!         theme.button.defaults.animation = Some(AnimationSpec::SPRING);
//!         ui.set_theme(theme);
//!         MyApp
//!     })?
//!     .run()?;
//! # Ok(())
//! # }
//! ```

pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod handle;
mod input;
mod native;
mod runtime;
mod window;
mod window_set;

use crate::gpu::device::power_preference::PowerPreference;
use std::marker::PhantomData;
use std::time::Instant;

use glam::UVec2;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::app::App;
use crate::common::platform::PLATFORM;
use crate::display;
use crate::gpu::surface::surface_manager::SurfaceManager;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::handle::{HostHandle, MainTask, UserEvent};
use crate::host::winit::runtime::WinitRuntime;
use crate::host::winit::window::FramePresent;
use crate::text::font_scope::FontScope;
use crate::ui::Ui;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use std::fmt;
use winit::error::EventLoopError;

type AppFactory<T> = Box<dyn FnOnce(&mut Ui, HostHandle<T>) -> T>;

/// What [`WinitHostBuilder::build`] stashes for the first `resumed`: the bootstrap window's token and config, and the app factory. Window, GPU and app construction wait for winit's event loop.
pub(super) struct Bootstrap<T: 'static> {
    pub(super) token: WindowToken,
    pub(super) config: WinitHostConfig,
    pub(super) create_app: Option<AppFactory<T>>,
    pub(super) pending_tasks: Vec<MainTask<T>>,
}

impl<T: 'static> fmt::Debug for Bootstrap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl<T: 'static> fmt::Debug for HostPhase<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap(bootstrap) => f.debug_tuple("Bootstrap").field(bootstrap).finish(),
            Self::Running(runtime) => f.debug_tuple("Running").field(runtime).finish(),
            Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
        }
    }
}

/// Top-level winit-driven runtime. Owns the app `T: App` and calls its update/record lifecycle once per redraw, per window. `HostPhase` makes bootstrap and running ownership mutually exclusive.
pub struct WinitHost<T: 'static> {
    phase: HostPhase<T>,
    event_loop: Option<EventLoop<UserEvent<T>>>,
    proxy: EventLoopProxy<UserEvent<T>>,
}

impl<T: 'static> fmt::Debug for WinitHost<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WinitHost")
            .field("phase", &self.phase)
            .field("event_loop", &self.event_loop.is_some())
            .finish_non_exhaustive()
    }
}

/// Startup configuration for [`WinitHost`].
#[derive(Debug)]
#[must_use]
pub struct WinitHostBuilder<T> {
    first_token: WindowToken,
    config: WinitHostConfig,
    marker: PhantomData<fn() -> T>,
}

impl<T> WinitHostBuilder<T>
where
    T: App + 'static,
{
    /// Set the bootstrap window's configuration.
    pub fn window(mut self, window: WindowConfig) -> Self {
        self.config.window = window;
        self
    }

    /// Set the bootstrap window's title; shorthand for the field of [`Self::window`] nearly every app sets.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.config.window.title = title.into();
        self
    }

    /// Which faces every window shapes against. Defaults to [`FontScope::System`] for OS glyph fallback; see [`OffscreenHostBuilder::fonts`](crate::OffscreenHostBuilder::fonts).
    pub const fn fonts(mut self, scope: FontScope) -> Self {
        self.config.fonts = scope;
        self
    }

    /// Start every window with `vsync`, avoiding a first-frame swapchain rebuild.
    pub const fn vsync(mut self, vsync: Vsync) -> Self {
        self.config.vsync = vsync;
        self
    }

    /// Adapter power preference. `LowPower` by default, unlike the headless paths (`HighPerformance`).
    pub const fn power_preference(mut self, preference: PowerPreference) -> Self {
        self.config.power_preference = preference;
        self
    }

    /// Opt into GPU timestamp and pipeline-statistics collection; off by default for its per-frame readback.
    pub const fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.config.collect_gpu_stats = collect;
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels. On by default; turn off for continuous position animation.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.config.pixel_snap = pixel_snap;
        self
    }

    /// Create the event loop and runtime host; `create_app` runs on the first active callback.
    ///
    /// # Errors
    ///
    /// Returns an error when winit cannot create the event loop.
    pub fn build(
        self,
        create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static,
    ) -> Result<WinitHost<T>, WinitHostError> {
        let mut event_loop_builder = EventLoop::<UserEvent<T>>::with_user_event();
        // winit's default macOS menu binds ⌘Q to `terminate:`, killing the process before the app can veto `CloseRequested`. Drop it so ⌘Q arrives as an ordinary key event.
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
    /// Start configuring a host whose bootstrap window is `first_token`.
    pub fn builder(first_token: WindowToken) -> WinitHostBuilder<T> {
        WinitHostBuilder {
            first_token,
            config: WinitHostConfig::default(),
            marker: PhantomData,
        }
    }

    /// A cheap-to-clone, `Send` handle for cross-thread repaint requests and run-on-main scheduling.
    pub fn handle(&self) -> HostHandle<T> {
        HostHandle {
            proxy: self.proxy.clone(),
        }
    }

    /// Drive the event loop to completion.
    ///
    /// # Errors
    ///
    /// Returns event-loop failures and window, surface, adapter or device failures.
    pub fn run(mut self) -> Result<(), WinitHostError> {
        let event_loop = self.event_loop.take().expect("event loop already consumed");
        let event_loop_result = event_loop.run_app(&mut self);
        let failure = match self.phase {
            HostPhase::Failed(error) => Some(error),
            HostPhase::Bootstrap(_) | HostPhase::Running(_) => None,
        };
        finish_run(failure, event_loop_result)
    }

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
    event_loop_result: Result<(), EventLoopError>,
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
        if let Err(error) = runtime.drain_window_requests(event_loop) {
            self.fail(event_loop, error);
            return;
        }
        runtime.repaint_on_shared_change();
        runtime.schedule(event_loop, now);
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(runtime) = self.running() else {
            return;
        };
        let max_texture_dim = runtime.surfaces.max_texture_dim;
        let Some(slot) = runtime.slot_of_id(id) else {
            return;
        };
        let win = runtime.window(slot);

        if let WindowEvent::ModifiersChanged(modifiers) = &event {
            win.modifiers = modifiers.state();
        }
        let mut wants_repaint = false;
        let scale = win.translation_scale();
        let at = input::Translation {
            scale_factor: scale,
            modifiers: win.modifiers,
            platform: PLATFORM,
        };
        let trace = input::translate(&event, at, |ev| {
            wants_repaint |= win.on_input(ev).repaint_requested;
        });
        win.note_pointer(trace, scale);
        if wants_repaint {
            win.next = FramePresent::Immediate;
        }

        match event {
            WindowEvent::RedrawRequested => runtime.draw(slot),

            WindowEvent::CloseRequested => {
                // Flag it and force a frame instead of removing the window: `Window::frame` surfaces `Ui::close_requested` so the app can veto with `Ui::keep_open`.
                win.close_requested = true;
                win.next = FramePresent::Immediate;
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                win.system_scale = display::sanitize_system_scale(scale_factor);
                win.invalidate_system_facts();
                win.next = FramePresent::Immediate;
            }
            WindowEvent::Moved(_) => win.invalidate_system_facts(),
            // Windows reports a minimize as a zero-size resize and sends no `Occluded`; treat it as hidden.
            WindowEvent::Resized(new) if new.width == 0 || new.height == 0 => {
                win.set_minimized(true);
            }
            WindowEvent::Resized(new) => {
                if win.set_minimized(false) {
                    win.next = FramePresent::Immediate;
                }
                win.note_resized();
                let size = SurfaceManager::clamp_extent(
                    max_texture_dim,
                    UVec2::new(new.width, new.height),
                );
                // Stash the new size only; `Window::frame` reconfigures the surface before the next acquire. Painting inline here lags on Wayland (it blocks on FIFO vsync), so defer to one `RedrawRequested` per loop tick.
                if win.surface.resize(size) {
                    win.next = FramePresent::Immediate;
                }
            }
            WindowEvent::Occluded(occluded) => {
                win.set_occluded(occluded);
                if !occluded {
                    // A window moved while hidden gets no `Moved`.
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
