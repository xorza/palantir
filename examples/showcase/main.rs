use std::sync::Arc;

use palantir::WgpuBackend;
use palantir::{Background, Button, Color, Configure, InputEvent, Panel, Sizing, Ui};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

mod alignment;
mod buttons;
mod clip;
mod disabled;
mod gap;
mod grid;
mod justify;
mod panels;
mod rounded;
mod scroll;
mod sizing;
mod spacing;
mod swatch;
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
    ("rounded clip", rounded::build),
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
    /// Set when the swapchain's contents are stale — acquire failed
    /// (Occluded / Timeout / Validation / Outdated / Lost) or surface
    /// was just reconfigured. Consumed at the top of `draw()` to
    /// invalidate damage's prev-frame snapshot, forcing the next
    /// `compute` to return `Full` instead of `Skip` against an
    /// unpainted backbuffer. Initial `true` so frame 1 rewinds
    /// explicitly rather than relying on damage's first-frame heuristic.
    new_surface: bool,
    active: usize,
    fps_window_start: std::time::Instant,
    fps_window_frames: u32,
    /// Host-side repaint gate. Set on input / resize / scale change /
    /// surface failure / `Occluded(false)`. Cleared at the top of
    /// `draw()`. Read in `about_to_wait` to decide whether to ask
    /// winit for a redraw.
    repaint_requested: bool,
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
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut backend = WgpuBackend::new(device.clone(), queue.clone(), format);

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
            new_surface: true,
            active: 0,
            fps_window_start: std::time::Instant::now(),
            fps_window_frames: 0,
            repaint_requested: true,
        });
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(state) = self.state.as_ref() else {
            return;
        };

        if state.repaint_requested {
            state.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        if let Some(ev) = InputEvent::from_winit(&event, state.display.scale_factor) {
            state.ui.on_input(ev);
            state.repaint_requested = true;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.display.scale_factor = scale_factor as f32;
                state.repaint_requested = true;
                state.new_surface = true;
            }
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
                state.display.physical = glam::UVec2::new(state.config.width, state.config.height);
                state.new_surface = true;
                state.repaint_requested = true;
            }
            WindowEvent::RedrawRequested => state.draw(),
            WindowEvent::Occluded(false) => state.repaint_requested = true,
            _ => {}
        }
    }
}

impl State {
    fn draw(&mut self) {
        self.fps_window_frames += 1;
        let elapsed = self.fps_window_start.elapsed();
        if elapsed.as_secs() >= 1 {
            let _fps = self.fps_window_frames as f64 / elapsed.as_secs_f64();
            // println!("fps: {_fps:.1}");
            self.fps_window_start = std::time::Instant::now();
            self.fps_window_frames = 0;
        }

        self.repaint_requested = false;

        if self.new_surface {
            self.ui.invalidate_prev_frame();
            self.new_surface = false;
        }

        self.ui.begin_frame(self.display);
        build_root(&mut self.ui, &mut self.active);
        let clear = self.ui.theme.window_clear;
        let frame_out = self.ui.end_frame();

        if frame_out.can_skip_rendering() {
            return;
        }

        use wgpu::CurrentSurfaceTexture::*;

        let frame = match self.surface.get_current_texture() {
            Success(f) => f,
            Suboptimal(_) | Outdated | Lost => {
                self.surface.configure(&self.device, &self.config);
                self.new_surface = true;
                self.repaint_requested = true;
                return;
            }
            Timeout | Validation => {
                self.new_surface = true;
                self.repaint_requested = true;
                return;
            }
            Occluded => {
                self.new_surface = true;
                return;
            }
        };

        self.backend.submit(&frame.texture, clear, frame_out);

        frame.present();
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
            // all on one line. Active button is rendered with the
            // hovered-state fill so it reads as "selected" — minimal
            // override on top of the default theme.
            Panel::wrap_hstack()
                .gap(6.0)
                .line_gap(6.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    let active_style = active_toolbar_button(&ui.theme.button);
                    for (i, (label, _)) in SHOWCASES.iter().enumerate() {
                        let mut btn = Button::new().with_id(*label).label(*label);
                        if i == *active {
                            btn = btn.style(active_style.clone());
                        }
                        if btn.show(ui).clicked() {
                            *active = i;
                        }
                    }
                });

            // Central panel: hosts the selected showcase. Uses palette
            // `surface` + `border` so the showcase cards sit visually
            // contained against the window's `bg`.
            Panel::zstack()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .background(Background {
                    fill: Color::hex(0x343434),
                    stroke: Some(palantir::Stroke {
                        width: 1.0,
                        color: Color::hex(0x363636),
                    }),
                    radius: palantir::Corners::all(8.0),
                })
                .show(ui, |ui| {
                    let (_, build_fn) = SHOWCASES[*active];
                    build_fn(ui);
                });
        });
}

/// Build a one-off ButtonTheme that highlights the active toolbar
/// entry: copy the default theme, swap the `normal` slot to use the
/// `hovered` background. Pressed / disabled / hovered fall through to
/// the defaults.
fn active_toolbar_button(default: &palantir::ButtonTheme) -> palantir::ButtonTheme {
    palantir::ButtonTheme {
        normal: default.hovered.clone(),
        ..default.clone()
    }
}
