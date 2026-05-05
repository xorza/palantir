use std::sync::Arc;

use palantir::WgpuBackend;
use palantir::{
    Background, Button, Color, Configure, Corners, InputEvent, Panel, Sizing, Stroke, Ui,
};
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
mod scroll;
mod sizing;
mod spacing;
mod text;
mod text_edit;
mod text_zorder;
mod transform;
mod visibility;
mod wrap;

/// Each showcase: a label for the toolbar button, and a builder that fills the
/// central panel. Adding a new showcase = one line here + one new module.
type ShowcaseFn = fn(&mut Ui);

const SHOWCASES: &[(&str, ShowcaseFn)] = &[
    ("text", text::build),
    ("text layouts", text::build_layouts),
    ("text edit", text_edit::build),
    ("z-order", text_zorder::build),
    ("panels", panels::build),
    ("scroll", scroll::build),
    ("wrap", wrap::build),
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
    ui: Ui,
    display: palantir::Display,
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

        let mut ui = Ui::new();
        // Showcase cards are dark — flip the scrollbar thumb to a
        // light translucent fill so it shows up against them.
        ui.theme.scrollbar = palantir::ScrollbarTheme {
            thumb: Color::rgba(1.0, 1.0, 1.0, 0.55),
            thumb_hover: Color::rgba(1.0, 1.0, 1.0, 0.75),
            thumb_active: Color::rgba(1.0, 1.0, 1.0, 0.9),
            ..Default::default()
        };
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
            active: 0,
        });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Pure event-driven: only wake to redraw when the UI itself
        // says something changed. Initial frame still needs a kick
        // because nothing has happened yet (no input, no resize).
        let needs_paint = match self.state.as_ref() {
            Some(state) => state.first_paint || state.ui.should_repaint(),
            None => false,
        };
        if needs_paint && let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Feed input first so `Ui::should_repaint` reflects this event
        // by the time we decide whether to schedule a redraw below.
        if let Some(ev) = InputEvent::from_winit(&event, state.display.scale_factor) {
            state.ui.on_input(ev);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
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
                state.draw();
            }
            WindowEvent::RedrawRequested => state.draw(),
            // Other events (cursor / button) flow through `on_input`
            // above, which sets the repaint gate. `about_to_wait`
            // turns that into a `request_redraw` call. No need to
            // handle each variant here — the gate is the single
            // source of truth.
            _ => {
                if state.ui.should_repaint() {
                    state.window.request_redraw();
                }
            }
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
        self.ui.begin_frame(self.display);
        build_root(&mut self.ui, &mut self.active);
        let frame_out = self.ui.end_frame();
        self.backend
            .submit(&frame.texture, Color::rgb(0.08, 0.08, 0.10), frame_out);

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
            // Toolbar: one button per showcase. WrapHStack so the buttons
            // wrap to a new row when the window is too narrow to fit them
            // all on one line. Active button is highlighted.
            Panel::wrap_hstack()
                .gap(6.0)
                .line_gap(6.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    for (i, (label, _)) in SHOWCASES.iter().enumerate() {
                        let is_active = i == *active;
                        let style = if is_active {
                            highlight_button_style()
                        } else {
                            outlined_button_style()
                        };
                        let r = Button::new()
                            .with_id(*label)
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
                .background(Background {
                    fill: Color::rgb(0.12, 0.14, 0.18),
                    stroke: Some(Stroke {
                        width: 1.0,
                        color: Color::rgb(0.25, 0.30, 0.40),
                    }),
                    radius: Corners::all(8.0),
                })
                .show(ui, |ui| {
                    let (_, build_fn) = SHOWCASES[*active];
                    build_fn(ui);
                });
        });
}

fn outlined_button_style() -> palantir::ButtonTheme {
    use palantir::{ButtonStyle, ButtonTheme, TextStyle};
    let stroke = Some(Stroke {
        width: 1.0,
        color: Color::rgb(0.4, 0.5, 0.7),
    });
    let bg = |fill, stroke| Background {
        fill,
        stroke,
        radius: Corners::all(4.0),
    };
    ButtonTheme {
        normal: ButtonStyle {
            background: Some(bg(Color::TRANSPARENT, stroke)),
            text: Some(TextStyle::default().with_color(Color::rgb(0.85, 0.88, 0.95))),
        },
        hovered: ButtonStyle {
            background: Some(bg(Color::rgba(0.4, 0.5, 0.7, 0.15), stroke)),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        pressed: ButtonStyle {
            background: Some(bg(Color::rgba(0.4, 0.5, 0.7, 0.30), stroke)),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        disabled: ButtonStyle {
            background: Some(bg(
                Color::TRANSPARENT,
                Some(Stroke {
                    width: 1.0,
                    color: Color::rgba(0.4, 0.5, 0.7, 0.35),
                }),
            )),
            text: Some(TextStyle::default().with_color(Color::rgba(0.85, 0.88, 0.95, 0.45))),
        },
    }
}

fn highlight_button_style() -> palantir::ButtonTheme {
    use palantir::{ButtonStyle, ButtonTheme, TextStyle};
    let s = outlined_button_style();
    let stroke = s.normal.background.and_then(|b| b.stroke);
    ButtonTheme {
        normal: ButtonStyle {
            background: Some(Background {
                fill: Color::rgba(0.4, 0.5, 0.7, 0.45),
                stroke,
                radius: Corners::all(4.0),
            }),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        ..s
    }
}
