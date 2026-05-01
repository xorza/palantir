use std::sync::Arc;
use std::time::{Duration, Instant};

use palantir::renderer::{ComposeParams, Pipeline, WgpuBackend};
use palantir::{Button, Color, Configure, InputEvent, Panel, Rect, Sizing, Stroke, Styled, Ui};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

mod alignment;
mod buttons;
mod clip;
mod disabled;
mod gap;
mod grid;
mod justify;
mod panels;
mod sizing;
mod spacing;
mod text;
mod transform;
mod visibility;

/// Each showcase: a label for the toolbar button, and a builder that fills the
/// central panel. Adding a new showcase = one line here + one new module.
type ShowcaseFn = fn(&mut Ui);

const SHOWCASES: &[(&str, ShowcaseFn)] = &[
    ("text", text::build),
    ("text layouts", text::build_layouts),
    ("panels", panels::build),
    ("grid", grid::build),
    ("sizing", sizing::build),
    ("alignment", alignment::build),
    ("justify", justify::build),
    ("clip", clip::build),
    ("transform", transform::build),
    ("visibility", visibility::build),
    ("disabled", disabled::build),
    ("gap", gap::build),
    ("spacing", spacing::build),
    ("buttons", buttons::build),
];

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
    pipeline: Pipeline,
    ui: Ui,
    first_paint: bool,
    active: usize,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("palantir showcase"))
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

        let mut backend = WgpuBackend::new(device.clone(), queue.clone(), format);

        let mut ui = Ui::new();
        ui.set_scale_factor(window.scale_factor() as f32);
        ui.set_pixel_snap(true);
        let cosmic = palantir::text::share(palantir::text::CosmicMeasure::with_bundled_fonts());
        ui.set_cosmic(cosmic.clone());
        backend.set_cosmic(cosmic);

        window.request_redraw();
        self.state = Some(State {
            window,
            surface,
            device,
            config,
            backend,
            pipeline: Pipeline::new(),
            ui,
            first_paint: false,
            active: 0,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

        if let Some(ev) = InputEvent::from_winit(&event, state.ui.scale_factor()) {
            state.ui.on_input(ev);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.ui.set_scale_factor(scale_factor as f32);
                state.window.request_redraw();
            }
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
                state.draw();
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
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return;
            }
            Timeout => {
                self.window.request_redraw();
                return;
            }
            Occluded => return,
            Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let scale = self.ui.scale_factor();
        let w_logical = self.config.width as f32 / scale;
        let h_logical = self.config.height as f32 / scale;

        self.ui.begin_frame();
        build_root(&mut self.ui, &mut self.active);
        self.ui.layout(Rect::new(0.0, 0.0, w_logical, h_logical));
        self.ui.end_frame();

        let buffer = self.pipeline.build(
            self.ui.tree(),
            self.ui.layout_result(),
            self.ui.cascades(),
            self.ui.theme.disabled_dim,
            &ComposeParams {
                viewport_logical: [w_logical, h_logical],
                scale,
                pixel_snap: self.ui.pixel_snap(),
            },
        );
        self.backend
            .submit(&view, Color::rgb(0.08, 0.08, 0.10), buffer);

        frame.present();
        if !self.first_paint {
            self.first_paint = true;
        }
    }
}

fn build_root(ui: &mut Ui, active: &mut usize) {
    Panel::vstack()
        .padding(12.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Toolbar: one button per showcase. The active one is highlighted.
            Panel::hstack()
                .gap(6.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    for (i, (label, _)) in SHOWCASES.iter().enumerate() {
                        let is_active = i == *active;
                        let style = if is_active {
                            highlight_button_style()
                        } else {
                            outlined_button_style()
                        };
                        let r = Button::with_id(*label)
                            .label(*label)
                            .style(style)
                            .padding((10.0, 6.0, 10.0, 6.0))
                            .show(ui);
                        if r.clicked() {
                            *active = i;
                        }
                    }
                });

            // Central panel: the selected showcase fills the rest.
            Panel::zstack()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .fill(Color::rgb(0.12, 0.14, 0.18))
                .stroke(Stroke {
                    width: 1.0,
                    color: Color::rgb(0.25, 0.30, 0.40),
                })
                .radius(8.0)
                .show(ui, |ui| {
                    let (_, build_fn) = SHOWCASES[*active];
                    build_fn(ui);
                });
        });
}

fn outlined_button_style() -> palantir::ButtonStyle {
    use palantir::{ButtonStyle, Corners, Visuals};
    let stroke = Some(Stroke {
        width: 1.0,
        color: Color::rgb(0.4, 0.5, 0.7),
    });
    ButtonStyle {
        normal: Visuals {
            fill: Color::TRANSPARENT,
            stroke,
            text: Color::rgb(0.85, 0.88, 0.95),
        },
        hovered: Visuals {
            fill: Color::rgba(0.4, 0.5, 0.7, 0.15),
            stroke,
            text: Color::WHITE,
        },
        pressed: Visuals {
            fill: Color::rgba(0.4, 0.5, 0.7, 0.30),
            stroke,
            text: Color::WHITE,
        },
        disabled: Visuals {
            fill: Color::TRANSPARENT,
            stroke: Some(Stroke {
                width: 1.0,
                color: Color::rgba(0.4, 0.5, 0.7, 0.35),
            }),
            text: Color::rgba(0.85, 0.88, 0.95, 0.45),
        },
        radius: Corners::all(4.0),
    }
}

fn highlight_button_style() -> palantir::ButtonStyle {
    use palantir::{ButtonStyle, Visuals};
    let s = outlined_button_style();
    ButtonStyle {
        normal: Visuals {
            fill: Color::rgba(0.4, 0.5, 0.7, 0.45),
            stroke: s.normal.stroke,
            text: Color::WHITE,
        },
        ..s
    }
}
