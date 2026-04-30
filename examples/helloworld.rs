use std::sync::Arc;

use palantir::{Button, Color, HStack, Rect, Sizing, Ui, layout, renderer::Renderer};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,palantir=debug,helloworld=debug")),
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            tracing::debug!("resumed: state already initialized, skipping");
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
        window.request_redraw();
        tracing::debug!("initial redraw requested");
        // Poll until the first paint succeeds; macOS may report Occluded with no follow-up event.
        event_loop.set_control_flow(ControlFlow::Poll);
        self.state = Some(State {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            ui: Ui::new(),
            first_paint: false,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_ref()
            && !state.first_paint
        {
            state.window.request_redraw();
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                tracing::info!(?new, max, "resized");
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                tracing::debug!("redraw requested");
                state.draw();
            }
            _ => {}
        }
    }
}

impl State {
    fn draw(&mut self) {
        use wgpu::CurrentSurfaceTexture::*;
        let acquired = self.surface.get_current_texture();
        tracing::debug!(?acquired, "acquired surface texture");
        let frame = match acquired {
            Success(f) | Suboptimal(f) => f,
            Outdated | Lost => {
                tracing::warn!("surface outdated/lost — reconfiguring and retrying");
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return;
            }
            Timeout => {
                tracing::warn!("surface timeout — retrying");
                self.window.request_redraw();
                return;
            }
            Occluded => {
                tracing::debug!("occluded — skipping frame");
                return;
            }
            Validation => {
                tracing::error!("validation error on get_current_texture");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let w = self.config.width as f32;
        let h = self.config.height as f32;

        self.ui.begin_frame();
        build_ui(&mut self.ui);
        let root = self.ui.root();
        layout::run(&mut self.ui.tree, root, Rect::new(0.0, 0.0, w, h));

        self.renderer.render(
            &self.device,
            &self.queue,
            &view,
            [w, h],
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

fn build_ui(ui: &mut Ui) {
    HStack::new().padding(16.0).show(ui, |ui| {
        Button::new()
            .label("Hello")
            .size((Sizing::Fill, Sizing::Hug))
            .min_size((120.0, 60.0))
            .margin((0.0, 0.0, 8.0, 0.0))
            .show(ui);
        Button::new()
            .label("World")
            .size((Sizing::Fixed(0.3), Sizing::Hug))
            .min_size((0.0, 80.0))
            .margin((4.0, 24.0, 32.0, 0.0))
            .show(ui);
    });
}
