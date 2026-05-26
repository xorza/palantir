//! `WinitHost` — wraps [`Host`] with a winit window, surface, and the
//! [`ApplicationHandler`] event-loop glue. Owns everything below the
//! user's app: window creation, swapchain config, resize / scale /
//! occlusion handling, and the `FramePresent` scheduling state machine.
//!
//! The caller-supplied app implements the [`App`] trait (just
//! `frame(&mut self, ui: &mut Ui)`, run once per redraw). The app itself
//! is built by a closure handed to [`WinitHost::new`], invoked once the
//! `Ui` + [`HostHandle`] are ready (before the first frame) — so startup
//! wiring (theme tweaks, restoring persisted state, stashing the handle)
//! happens there.
//!
//! Usage:
//!
//! ```ignore
//! struct MyApp;
//! impl palantir::App for MyApp {
//!     fn frame(&mut self, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::new(WinitHostConfig::new("title"), |ui, _handle| {
//!     ui.theme.button.anim = Some(AnimSpec::SPRING);
//!     MyApp
//! })
//! .run();
//! ```

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::host::{FramePresent, Host, HostConfig};
use crate::input::InputEvent;
use crate::text::TextShaper;
use crate::ui::Ui;

type MainTask = Box<dyn FnOnce(&mut Ui) -> bool + Send>;

/// Builds the caller's app once the `Ui` + [`HostHandle`] exist — handed
/// to [`WinitHost::new`] and invoked on the first `resumed`.
type AppBuilder<T> = Box<dyn FnOnce(&mut Ui, HostHandle) -> T>;

/// Events delivered to the host through [`HostHandle`] — cross-thread
/// pokes that the winit event loop turns into a redraw or a run-on-main
/// callback. Public only as the type parameter of `EventLoopProxy`;
/// construct via the methods on [`HostHandle`].
pub enum UserEvent {
    /// Wake the loop and request one redraw. Coalesced — many in a
    /// row collapse to one frame.
    Repaint,
    /// Run a closure on the main (event-loop) thread with `&mut Ui`,
    /// then request a redraw.
    RunOnMain(MainTask),
    /// Ask the event loop to exit at the next opportunity.
    Quit,
}

/// Thread-safe handle to a running [`WinitHost`]. Cheaply `Clone`; send
/// to background threads so they can poke the UI without owning it.
///
/// Obtain one via [`WinitHost::handle`] before calling `run`.
#[derive(Clone, Debug)]
pub struct HostHandle {
    proxy: EventLoopProxy<UserEvent>,
}

impl HostHandle {
    /// Request the host paint one frame. Cheap and lock-free; safe to
    /// call from any thread. Drops silently if the event loop has
    /// already exited.
    pub fn request_repaint(&self) {
        let _ = self.proxy.send_event(UserEvent::Repaint);
    }

    /// Schedule `f` to run on the main thread with `&mut Ui` before
    /// the next frame. Use for state mutations that aren't safe to
    /// perform off-thread (touching the recorder, the wgpu device,
    /// etc.). Return `true` from `f` to request a repaint, `false`
    /// to leave the present schedule unchanged.
    pub fn run_on_main(&self, f: impl FnOnce(&mut Ui) -> bool + Send + 'static) {
        let _ = self.proxy.send_event(UserEvent::RunOnMain(Box::new(f)));
    }

    /// Ask the host's event loop to exit. The current frame finishes;
    /// no further frames are scheduled.
    pub fn quit(&self) {
        let _ = self.proxy.send_event(UserEvent::Quit);
    }
}

/// How many frames may be in flight between CPU record and GPU
/// present. Maps 1:1 to wgpu's `desired_maximum_frame_latency`. Two
/// variants because the only sane choices on every supported platform
/// are 1 and 2 — extend the enum if a real use case for 3+ shows up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameLatency {
    /// 1 frame in flight — lowest input-to-photon latency, less
    /// pacing headroom. Default.
    Low,
    /// 2 frames in flight — smoother pacing under bursty frames at
    /// the cost of one extra frame of input lag.
    Buffered,
}

impl FrameLatency {
    fn frames(self) -> u32 {
        match self {
            FrameLatency::Low => 1,
            FrameLatency::Buffered => 2,
        }
    }
}

/// Tunables forwarded to winit + wgpu at startup. All fields are
/// optional with sensible defaults; override what you care about.
pub struct WinitHostConfig {
    pub title: String,
    /// Initial window inner size in logical pixels (DPI-independent).
    /// `None` lets the platform pick.
    pub inner_size: Option<LogicalSize<u32>>,
    /// Minimum window inner size in logical pixels. `None` = no floor.
    pub min_inner_size: Option<LogicalSize<u32>>,
    pub present_mode: wgpu::PresentMode,
    pub power_preference: wgpu::PowerPreference,
    /// How many frames may be in flight on the GPU at once. See
    /// [`FrameLatency`].
    pub max_frame_latency: FrameLatency,
    /// GPU texture cache budget; eviction kicks in past this. See
    /// [`DEFAULT_IMAGE_BUDGET_BYTES`](crate::DEFAULT_IMAGE_BUDGET_BYTES).
    pub image_budget_bytes: u64,
    /// Opt into GPU instrumentation (timestamp + pipeline-statistics
    /// queries). Off by default because the per-frame readback
    /// round-trip is non-trivial.
    pub collect_gpu_stats: bool,
}

impl Default for WinitHostConfig {
    fn default() -> Self {
        Self {
            title: String::from("palantir"),
            inner_size: None,
            min_inner_size: None,
            present_mode: wgpu::PresentMode::AutoVsync,
            power_preference: wgpu::PowerPreference::LowPower,
            max_frame_latency: FrameLatency::Low,
            image_budget_bytes: HostConfig::default().image_budget_bytes,
            collect_gpu_stats: false,
        }
    }
}

impl WinitHostConfig {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// The caller-supplied app. `WinitHost` builds it via the closure passed
/// to [`WinitHost::run`] once the `Ui` and [`HostHandle`] exist (after
/// device + surface are up, before the first frame), then calls
/// [`App::frame`] once per redraw.
pub trait App {
    /// Build one frame: implementors mutate themselves and emit widgets.
    fn frame(&mut self, ui: &mut Ui);
}

/// Top-level winit-driven palantir runtime. Owns the caller-supplied
/// app `T: App` for convenience (RAII lifetime, no `Rc<RefCell<>>`
/// to manage) and calls `T::frame` once per redraw with `&mut Ui`.
pub struct WinitHost<T> {
    config: WinitHostConfig,
    /// `None` until `resumed` builds the `Ui` and runs the app builder;
    /// the app lands here. The app can't exist before the `Ui` does, so
    /// construction is necessarily deferred.
    app: Option<T>,
    /// The caller's app builder, set by [`WinitHost::new`] and consumed
    /// by the first `resumed`. `None` after the build.
    app_builder: Option<AppBuilder<T>>,
    state: Option<RuntimeState>,
    event_loop: Option<EventLoop<UserEvent>>,
    proxy: EventLoopProxy<UserEvent>,
}

struct RuntimeState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    host: Host,
    scale_factor: f32,
    /// Host-side scheduling state. Reset at the top of `draw` from the
    /// `FramePresent` the frame returned; re-armed to `Immediate` by
    /// input, resize, surface loss, occlusion, and animation tickers.
    next: FramePresent,
}

impl<T> WinitHost<T>
where
    T: App + 'static,
{
    /// `build` constructs the app once the `Ui` + [`HostHandle`] are
    /// ready (after device + surface are up, before the first frame) —
    /// do startup wiring (theme tweaks, restoring persisted state,
    /// stashing the handle) inside it. It runs on the first `resumed`,
    /// not here.
    pub fn new(
        config: WinitHostConfig,
        build: impl FnOnce(&mut Ui, HostHandle) -> T + 'static,
    ) -> Self {
        // EventLoop is built up front so `handle()` can hand out a
        // proxy before `run()` is called — that's the whole point of
        // letting threads spawn knowing where to send their pokes.
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .expect("event loop");
        let proxy = event_loop.create_proxy();
        Self {
            config,
            app: None,
            app_builder: Some(Box::new(build)),
            state: None,
            event_loop: Some(event_loop),
            proxy,
        }
    }

    /// Return a cheap-to-clone, `Send` handle for cross-thread
    /// repaint requests and run-on-main scheduling. Stable for the
    /// lifetime of the host — call before `run()` and ship the
    /// handle to worker threads.
    pub fn handle(&self) -> HostHandle {
        HostHandle {
            proxy: self.proxy.clone(),
        }
    }

    /// Drive the (already-constructed) event loop to completion.
    pub fn run(mut self) {
        let event_loop = self.event_loop.take().expect("event loop already consumed");
        event_loop.run_app(&mut self).expect("run app");
    }

    fn draw(&mut self) {
        let Self { state, app, .. } = self;
        let (Some(rt), Some(app)) = (state.as_mut(), app.as_mut()) else {
            return;
        };
        let window = rt.window.clone();
        rt.next = rt.host.frame(
            &rt.surface,
            &rt.config,
            rt.scale_factor,
            |ui| app.frame(ui),
            || window.pre_present_notify(),
        );
    }
}

impl<T> ApplicationHandler<UserEvent> for WinitHost<T>
where
    T: App + 'static,
{
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        if matches!(event, UserEvent::Quit) {
            event_loop.exit();
            return;
        }
        let Some(rt) = self.state.as_mut() else {
            return;
        };
        let repaint = match event {
            UserEvent::Repaint => true,
            UserEvent::RunOnMain(task) => task(&mut rt.host.ui),
            UserEvent::Quit => unreachable!(),
        };
        if repaint {
            rt.next = FramePresent::Immediate;
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let cfg = &self.config;
        let mut attrs = Window::default_attributes().with_title(cfg.title.clone());
        if let Some(size) = cfg.inner_size {
            attrs = attrs.with_inner_size(size);
        }
        if let Some(size) = cfg.min_inner_size {
            attrs = attrs.with_min_inner_size(size);
        }
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: cfg.power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request adapter");

        // Caller-driven opt-in via `WinitHostConfig::collect_gpu_stats`
        // — see field doc. When off, none of the timing-query features
        // are requested, so the per-frame `resolve_query_set` +
        // `map_async` + `device.poll(Poll)` + readback are all
        // dead-stripped. When on, the three optional features
        // degrade independently per adapter advertisement: the
        // intersection with `adapter.features()` below drops bits
        // the adapter doesn't support. `TIMESTAMP_QUERY` alone →
        // pass begin/end only; `+ TIMESTAMP_QUERY_INSIDE_PASSES` →
        // per-batch attribution; `+ PIPELINE_STATISTICS_QUERY` →
        // vert/frag invocation counts.
        let timing_features = if cfg.collect_gpu_stats {
            wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY
        } else {
            wgpu::Features::empty()
        };
        // `IMMEDIATES` carries the text backend's atlas-size params
        // (`renderer::backend::text::Params`) — register-mapped per-pass instead
        // of a uniform buffer + bind group. Unconditionally required
        // because every Metal/Vulkan/DX12 adapter exposes it
        // (WebGPU-only adapters are off-target for palantir).
        let required_features = (adapter.features() & timing_features) | wgpu::Features::IMMEDIATES;
        let mut required_limits = wgpu::Limits::default().using_resolution(adapter.limits());
        // 16 bytes covers `renderer::backend::text::Params` (vec2<u32>) with room
        // for the WGSL 16-byte uniform-struct rounding.
        required_limits.max_immediate_size = required_limits.max_immediate_size.max(16);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("palantir.device"),
            required_features,
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        // Color pipeline assumes an sRGB swapchain target — see the
        // colour section of CLAUDE.md. Non-sRGB would skip the GPU
        // linear→sRGB encode and silently darken every paint.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .expect("no sRGB-capable surface format");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: cfg.present_mode,
            alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                caps.alpha_modes[0]
            },
            view_formats: vec![],
            desired_maximum_frame_latency: cfg.max_frame_latency.frames(),
        };
        // `Host::frame` configures the surface lazily when it spots
        // `(config.width, config.height)` differing from the last
        // configured pair — first paint hits that path because the
        // baseline is `None`. No eager configure here.

        // Forward `collect_gpu_stats` through to the Host so the
        // backend's `GpuTimings` actually runs. Without this the
        // adapter would advertise the feature (we requested it above)
        // but the backend would never opt in — `last_pass_ms()` would
        // always be `None` even though the device supports timing.
        let mut host = Host::with_options(
            device.clone(),
            queue.clone(),
            format,
            TextShaper::with_bundled_fonts(),
            HostConfig {
                image_budget_bytes: cfg.image_budget_bytes,
                collect_gpu_stats: cfg.collect_gpu_stats,
            },
        );
        // Build the app now that the `Ui` exists. `resumed` can fire
        // again after a suspend; only construct on the first pass so a
        // resume doesn't wipe accumulated app state.
        if self.app.is_none() {
            let handle = HostHandle {
                proxy: self.proxy.clone(),
            };
            let build = self.app_builder.take().expect("app builder consumed");
            self.app = Some(build(&mut host.ui, handle));
        }
        let scale_factor = window.scale_factor() as f32;

        // `next: Immediate` below makes `about_to_wait` request the
        // first redraw — no need to call `request_redraw()` here.
        self.state = Some(RuntimeState {
            window,
            surface,
            device,
            config,
            host,
            scale_factor,
            next: FramePresent::Immediate,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(rt) = self.state.as_ref() else {
            return;
        };

        // `At(t)` with `t <= now` collapses to `Immediate` — `WaitUntil`
        // would fire instantly and loop, so just request the redraw.
        // That fold lets us drop the `new_events` `ResumeTimeReached`
        // rewrite: a deadline-driven wake naturally lands here next.
        let next = match rt.next {
            FramePresent::At(t) if t <= std::time::Instant::now() => FramePresent::Immediate,
            other => other,
        };
        match next {
            FramePresent::Immediate => {
                rt.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            FramePresent::At(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            FramePresent::Idle => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(rt) = self.state.as_mut() else {
            return;
        };

        let mut wants_repaint = false;
        InputEvent::from_winit(&event, rt.scale_factor, |ev| {
            wants_repaint |= rt.host.ui.on_input(ev).requests_repaint;
        });
        if wants_repaint {
            rt.next = FramePresent::Immediate;
        }

        match event {
            WindowEvent::RedrawRequested => self.draw(),

            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                rt.scale_factor = scale_factor as f32;
                rt.next = FramePresent::Immediate;
            }
            WindowEvent::Resized(new) => {
                let max = rt.device.limits().max_texture_dimension_2d;
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
            WindowEvent::Occluded(occluded) => {
                rt.host.set_occluded(occluded);
                if !occluded {
                    rt.next = FramePresent::Immediate;
                }
            }

            _ => {}
        }
    }
}
