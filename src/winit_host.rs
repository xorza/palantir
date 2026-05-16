//! `WinitHost` — wraps [`Host`] with a winit window, surface, and the
//! [`ApplicationHandler`] event-loop glue. Owns everything below the
//! user's frame-builder closure: window creation, swapchain config,
//! resize / scale / occlusion handling, and the `FramePresent`
//! scheduling state machine.
//!
//! Usage:
//!
//! ```ignore
//! WinitHost::new("title", AppState::default(), |ui| build_ui(ui))
//!     .with_setup(|host| host.ui.theme.button.anim = Some(AnimSpec::SPRING))
//!     .run();
//! ```

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::host::{FramePresent, Host};
use crate::input::InputEvent;
use crate::ui::Ui;

type SetupFn<T> = Box<dyn FnOnce(&mut Ui<T>)>;

/// Top-level winit-driven palantir runtime. Generic over the app state
/// `T` (installed ambient on the `Ui` every frame) and the
/// frame-builder closure `F`.
pub struct WinitHost<T, F> {
    title: String,
    app: T,
    builder: F,
    setup: Option<SetupFn<T>>,
    state: Option<RuntimeState<T>>,
}

struct RuntimeState<T> {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    host: Host<T>,
    scale_factor: f32,
    /// Host-side scheduling state. Reset at the top of `draw` from the
    /// `FramePresent` the frame returned; re-armed to `Immediate` by
    /// input, resize, surface loss, occlusion, and animation tickers.
    next: FramePresent,
}

impl<T, F> WinitHost<T, F>
where
    T: 'static,
    F: FnMut(&mut Ui<T>),
{
    pub fn new(title: impl Into<String>, app: T, builder: F) -> Self {
        Self {
            title: title.into(),
            app,
            builder,
            setup: None,
            state: None,
        }
    }

    /// Run a one-shot configuration step against the freshly-built
    /// `Ui` (after device + surface are up). Use for theme tweaks
    /// and any other `Ui` mutation that needs to happen before the
    /// first frame.
    pub fn with_setup(mut self, setup: impl FnOnce(&mut Ui<T>) + 'static) -> Self {
        self.setup = Some(Box::new(setup));
        self
    }

    /// Construct the event loop and drive it to completion.
    pub fn run(mut self) {
        let event_loop = EventLoop::new().expect("event loop");
        event_loop.run_app(&mut self).expect("run app");
    }

    fn draw(&mut self) {
        let Self {
            state,
            app,
            builder,
            ..
        } = self;
        let Some(rt) = state.as_mut() else {
            return;
        };
        rt.next = rt
            .host
            .frame(&rt.surface, &rt.config, rt.scale_factor, app, |ui| {
                builder(ui)
            });
    }
}

impl<T, F> ApplicationHandler for WinitHost<T, F>
where
    T: 'static,
    F: FnMut(&mut Ui<T>),
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title(self.title.clone()))
                .expect("create window"),
        );
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("palantir.device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
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
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut host = Host::new(device.clone(), queue.clone(), format);
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
                rt.config.width = new.width.clamp(1, max);
                rt.config.height = new.height.clamp(1, max);
                rt.surface.configure(&rt.device, &rt.config);
                // Let `RedrawRequested` drive the actual paint —
                // most platforms emit one immediately after resize,
                // and painting inline here would double-draw.
                rt.next = FramePresent::Immediate;
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
