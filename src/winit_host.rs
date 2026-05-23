//! `WinitHost` — wraps [`Host`] with a winit window, surface, and the
//! [`ApplicationHandler`] event-loop glue. Owns everything below the
//! user's app: window creation, swapchain config, resize / scale /
//! occlusion handling, and the `FramePresent` scheduling state machine.
//!
//! The caller-supplied app implements the [`App`] trait — one method,
//! `frame(&mut self, ui: &mut Ui)`, called once per frame.
//!
//! Usage:
//!
//! ```ignore
//! struct MyApp;
//! impl palantir::App for MyApp {
//!     fn frame(&mut self, ui: &mut Ui) { /* build ui */ }
//! }
//! WinitHost::new(WinitHostConfig::new("title"), MyApp)
//!     .with_setup(|ui| ui.theme.button.anim = Some(AnimSpec::SPRING))
//!     .run();
//! ```

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::host::{FramePresent, Host, HostConfig};
use crate::input::InputEvent;
use crate::text::TextShaper;
use crate::ui::Ui;

type SetupFn = Box<dyn FnOnce(&mut Ui)>;

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
    /// [`crate::renderer::DEFAULT_IMAGE_BUDGET_BYTES`].
    pub image_budget_bytes: u64,
    /// Opt into GPU instrumentation (`wgpu::Features::TIMESTAMP_QUERY`,
    /// `+TIMESTAMP_QUERY_INSIDE_PASSES`, `+PIPELINE_STATISTICS_QUERY`).
    /// Features still degrade independently per adapter advertisement;
    /// this flag controls intent, not capability. Off by default —
    /// even on builds with the `internals` cargo feature, the host
    /// only collects stats when the caller explicitly asks for them.
    /// The per-frame cost is `resolve_query_set` + `map_async` +
    /// `device.poll(Poll)` + readback on every submit, so the price
    /// is non-trivial — flip it on for benches / debug overlays, off
    /// for production timing runs.
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
            image_budget_bytes: crate::renderer::DEFAULT_IMAGE_BUDGET_BYTES,
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

/// Per-frame user hook. `WinitHost` calls `frame` once per redraw with
/// the recording `Ui`; implementors mutate themselves and emit widgets.
pub trait App {
    fn frame(&mut self, ui: &mut Ui);
}

/// Top-level winit-driven palantir runtime. Owns the caller-supplied
/// app `T: App` for convenience (RAII lifetime, no `Rc<RefCell<>>`
/// to manage) and calls `T::frame` once per redraw with `&mut Ui`.
pub struct WinitHost<T> {
    config: WinitHostConfig,
    app: T,
    setup: Option<SetupFn>,
    state: Option<RuntimeState>,
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
    pub fn new(config: WinitHostConfig, app: T) -> Self {
        Self {
            config,
            app,
            setup: None,
            state: None,
        }
    }

    /// Run a one-shot configuration step against the freshly-built
    /// `Ui` (after device + surface are up). Use for theme tweaks
    /// and any other `Ui` mutation that needs to happen before the
    /// first frame.
    pub fn with_setup(mut self, setup: impl FnOnce(&mut Ui) + 'static) -> Self {
        self.setup = Some(Box::new(setup));
        self
    }

    /// Construct the event loop and drive it to completion.
    pub fn run(mut self) {
        let event_loop = EventLoop::new().expect("event loop");
        event_loop.run_app(&mut self).expect("run app");
    }

    fn draw(&mut self) {
        let Self { state, app, .. } = self;
        let Some(rt) = state.as_mut() else {
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

impl<T> ApplicationHandler for WinitHost<T>
where
    T: App + 'static,
{
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
        if let Some(setup) = self.setup.take() {
            setup(&mut host.ui);
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
