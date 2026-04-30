use std::sync::Arc;
use std::time::{Duration, Instant};

use palantir::{
    Button, ButtonStyle, Color, HStack, InputEvent, Layoutable, Rect, Sizing, Stroke, Ui, VStack,
    ZStack, layout, renderer::Renderer,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run app");
}

#[derive(Default)]
struct App {
    state: Option<State>,
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    ui: Ui,
    first_paint: bool,
    click_count: u32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("palantir"))
                .expect("create window"),
        );
        let size = window.inner_size();
        tracing::info!(?size, scale = window.scale_factor(), "window created");

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
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, format);

        tracing::info!(
            ?format,
            w = config.width,
            h = config.height,
            "surface configured"
        );

        let mut ui = Ui::new();
        ui.set_scale_factor(window.scale_factor() as f32);
        ui.set_pixel_snap(true);

        window.request_redraw();
        self.state = Some(State {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            ui,
            first_paint: false,
            click_count: 0,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // macOS may report Occluded for ~100ms after surface configure with no follow-up
        // event. Throttle the retry loop to ~60Hz until first paint succeeds.
        if let Some(state) = self.state.as_ref()
            && !state.first_paint
        {
            state.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Translate winit events to palantir-native events at the boundary, then
        // feed them in. Ui itself is winit-agnostic.
        if let Some(ev) = InputEvent::from_winit(&event, state.ui.scale_factor()) {
            state.ui.on_input(ev);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::info!(scale_factor, "scale factor changed");
                state.ui.set_scale_factor(scale_factor as f32);
                state.window.request_redraw();
            }
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                tracing::info!(?new, max, "resized");
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
                state.window.request_redraw();
            }
            WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. } => {
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => state.draw(),
            _ => {}
        }
    }
}

impl State {
    fn draw(&mut self) {
        use wgpu::CurrentSurfaceTexture::*;
        let frame = match self.surface.get_current_texture() {
            Success(f) | Suboptimal(f) => f,
            Outdated | Lost => {
                tracing::warn!("surface outdated/lost — reconfiguring");
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return;
            }
            Timeout => {
                self.window.request_redraw();
                return;
            }
            Occluded => return,
            Validation => {
                tracing::error!("validation error on get_current_texture");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let scale = self.ui.scale_factor();
        let w_logical = self.config.width as f32 / scale;
        let h_logical = self.config.height as f32 / scale;

        self.ui.begin_frame();
        build_ui(&mut self.ui, &mut self.click_count);
        let root = self.ui.root();
        layout::run(
            &mut self.ui.tree,
            root,
            Rect::new(0.0, 0.0, w_logical, h_logical),
        );
        self.ui.end_frame();

        let pixel_snap = self.ui.pixel_snap();
        self.renderer.render(
            &self.device,
            &self.queue,
            &view,
            [w_logical, h_logical],
            scale,
            pixel_snap,
            Color::rgb(0.08, 0.08, 0.10),
            &self.ui.tree,
        );

        frame.present();
        if !self.first_paint {
            tracing::info!("first paint succeeded");
            self.first_paint = true;
        }
    }
}

fn build_ui(ui: &mut Ui, clicks: &mut u32) {
    VStack::new()
        .padding(16.0)
        .size((Sizing::Fill, Sizing::Fill))
        .show(ui, |ui| {
            // Row 1: counter + reset buttons.
            HStack::new()
                .size((Sizing::Fill, Sizing::Hug))
                .show(ui, |ui| {
                    let counter = Button::new()
                        .label(format!("clicks: {clicks}"))
                        .size((Sizing::Fill, Sizing::Hug))
                        .min_size((120.0, 60.0))
                        .margin((0.0, 0.0, 8.0, 0.0))
                        .show(ui);
                    if counter.clicked() {
                        *clicks += 1;
                        tracing::info!(clicks = *clicks, "click");
                    }

                    let reset = Button::new()
                        .label("reset")
                        .style(ButtonStyle::outlined())
                        .size((Sizing::Fill, Sizing::Hug))
                        .min_size((0.0, 10.0))
                        .margin((4.0, 24.0, 32.0, 0.0))
                        .radius(4)
                        .show(ui);
                    if reset.clicked() {
                        *clicks = 0;
                        tracing::info!("reset");
                    }
                });

            // Row 2: ZStack with a tinted bg + a button that spills outside it via
            // negative margins. Demonstrates layered painting and CSS-style margin.
            ZStack::with_id("spill_demo")
                .size((Sizing::Fixed(280.0), Sizing::Fill))
                .padding(16.0)
                .margin((0.0, 24.0, 0.0, 0.0))
                .fill(Color::rgb(0.16, 0.20, 0.28))
                .stroke(Stroke {
                    width: 1.0,
                    color: Color::rgb(0.30, 0.36, 0.46),
                })
                .radius(12.0)
                .show(ui, |ui| {
                    Button::with_id("spiller")
                        .label("spilling")
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                        .margin((-24.0, -16.0, 0.0, 0.0))
                        .show(ui);
                });
        });
}
