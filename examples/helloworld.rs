use std::sync::Arc;
use std::time::{Duration, Instant};

use palantir::Align;
use palantir::WgpuBackend;
use palantir::{
    Background, Button, Color, Configure, Corners, InputEvent, Panel, Sizing, Stroke, Ui,
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
    config: wgpu::SurfaceConfiguration,
    backend: WgpuBackend,
    ui: Ui,
    display: palantir::Display,
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut backend = WgpuBackend::new(device.clone(), queue.clone(), format);

        tracing::info!(
            ?format,
            w = config.width,
            h = config.height,
            "surface configured"
        );

        let mut ui = Ui::new();
        let display = palantir::Display::from_physical(
            glam::UVec2::new(config.width, config.height),
            window.scale_factor() as f32,
        );
        let cosmic = palantir::share(palantir::CosmicMeasure::with_bundled_fonts());
        ui.set_cosmic(cosmic.clone());
        backend.set_cosmic(cosmic);

        window.request_redraw();
        self.state = Some(State {
            window,
            surface,
            device,
            config,
            backend,
            ui,
            display,
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
        if let Some(ev) = InputEvent::from_winit(&event, state.display.scale_factor) {
            state.ui.on_input(ev);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::info!(scale_factor, "scale factor changed");
                state.display.scale_factor = scale_factor as f32;
                state.ui.request_repaint();
                state.window.request_redraw();
            }
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
                state.display.physical = glam::UVec2::new(state.config.width, state.config.height);
                state.ui.request_repaint();
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

        self.ui.begin_frame(self.display);
        build_ui(&mut self.ui, &mut self.click_count);
        let frame_out = self.ui.end_frame();
        // Window background: palette `bg`.
        self.backend
            .submit(&frame.texture, Color::hex(0x252525), frame_out);

        frame.present();
        if !self.first_paint {
            tracing::info!("first paint succeeded");
            self.first_paint = true;
        }
    }
}

fn build_ui(ui: &mut Ui, clicks: &mut u32) {
    Panel::vstack()
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Row 1: counter + reset buttons.
            Panel::hstack()
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    let counter = Button::new()
                        .label(format!("clicks: {clicks}"))
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((120.0, 60.0))
                        .margin((0.0, 0.0, 8.0, 0.0))
                        .show(ui);
                    if counter.clicked() {
                        *clicks += 1;
                        tracing::info!(clicks = *clicks, "click");
                    }

                    let reset = Button::new()
                        .label("reset")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((0.0, 10.0))
                        .margin((4.0, 24.0, 32.0, 0.0))
                        .show(ui);
                    if reset.clicked() {
                        *clicks = 0;
                        tracing::info!("reset");
                    }
                });

            // Row 2: ZStack with a tinted bg + a button that spills outside it via
            // negative margins. Demonstrates layered painting and CSS-style margin.
            //
            //
            Panel::hstack()
                .size((Sizing::FILL, Sizing::FILL))
                .disabled(false)
                .show(ui, |ui| {
                    Panel::zstack()
                        .with_id("spill_demo")
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(16.0)
                        .margin(5)
                        .clip(true)
                        .background(Background {
                            fill: Color::hex(0x252525),
                            stroke: Some(Stroke {
                                width: 1.0,
                                color: Color::hex(0x363636),
                            }),
                            radius: Corners::all(12.0),
                        })
                        .show(ui, |ui| {
                            Button::new()
                                .with_id("spiller")
                                .label("spilling")
                                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                                .margin((-24.0, -16.0, 0.0, 0.0))
                                .show(ui);
                        });

                    Panel::zstack()
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(16.0)
                        .margin(5)
                        .background(Background {
                            fill: Color::hex(0x252525),
                            stroke: Some(Stroke {
                                width: 1.0,
                                color: Color::hex(0x363636),
                            }),
                            radius: Corners::all(12.0),
                        })
                        .show(ui, |ui| {
                            Button::new()
                                .align(Align::CENTER)
                                .label("centered")
                                .show(ui);
                        });
                });
        });
}
