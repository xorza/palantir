use std::sync::Arc;

use palantir::{Background, Button, Color, Configure, Host, InputEvent, Panel, Shadow, Sizing, Ui};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

mod showcase;
use showcase::{
    alignment, animations, bezier, buttons, clip, context_menu, disabled, drag, gap, gradients,
    grid, justify, lines, mesh, pan_zoom, pan_zoom_auto, panels, popup, scroll, shadow, sizing,
    spacing, text, text_edit, text_zorder, transform, visibility, wrap,
};

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
    ("pan+zoom", pan_zoom::build),
    (pan_zoom_auto::NAME, pan_zoom_auto::build),
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
    ("popup", popup::build),
    ("context menu", context_menu::build),
    ("animations", animations::build),
    ("mesh", mesh::build),
    ("lines", lines::build),
    ("bezier", bezier::build),
    ("drag", drag::build),
    ("gradients", gradients::build),
    ("shadow", shadow::build),
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
    host: Host,
    scale_factor: f32,
    active: usize,
    /// Host-side repaint gate. Cleared at top of `draw`; re-armed by
    /// input, resize, surface loss, occlusion, and animation tickers.
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

        let mut host = Host::new(device.clone(), queue.clone(), format);
        // Library default is no button animation (`anim = None`).
        // Showcase exists to demo the animation primitive — opt in.
        host.ui.theme.button.anim = None;
        host.ui.theme.button.anim = Some(palantir::AnimSpec::SPRING);
        let scale_factor = window.scale_factor() as f32;

        window.request_redraw();
        self.state = Some(State {
            window,
            surface,
            device,
            config,
            host,
            scale_factor,
            active: 0,
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

        if let WindowEvent::KeyboardInput {
            event:
                KeyEvent {
                    physical_key: PhysicalKey::Code(key),
                    state: ElementState::Pressed,
                    repeat: false,
                    ..
                },
            ..
        } = event
            && handle_debug_key(state, key)
        {
            state.repaint_requested = true;
        }

        if let Some(ev) = InputEvent::from_winit(&event, state.scale_factor) {
            state.host.ui.on_input(ev);
            state.repaint_requested = true;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.scale_factor = scale_factor as f32;
                state.repaint_requested = true;
            }
            WindowEvent::Resized(new) => {
                let max = state.device.limits().max_texture_dimension_2d;
                state.config.width = new.width.clamp(1, max);
                state.config.height = new.height.clamp(1, max);
                state.surface.configure(&state.device, &state.config);
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
        self.repaint_requested =
            self.host
                .frame_and_render(&self.surface, &self.config, self.scale_factor, |ui| {
                    build_ui(ui, &mut self.active)
                });
    }
}

/// F12 toggles damage-rect outlines; F10 toggles darken-undamaged;
/// F9 toggles the frame/FPS readout. Returns `true` if the key was
/// handled.
fn handle_debug_key(state: &mut State, key: KeyCode) -> bool {
    let overlay = &mut state.host.ui.debug_overlay;
    match key {
        KeyCode::F12 => {
            overlay.damage_rect = !overlay.damage_rect;
            eprintln!(
                "[F12] damage rect overlay: {}",
                if overlay.damage_rect { "on" } else { "off" }
            );
            true
        }
        KeyCode::F10 => {
            overlay.dim_undamaged = !overlay.dim_undamaged;
            eprintln!("[F10] darken undamaged: {}", overlay.dim_undamaged);
            true
        }
        KeyCode::F9 => {
            overlay.frame_stats = !overlay.frame_stats;
            eprintln!("[F9] frame stats: {}", overlay.frame_stats);
            true
        }
        _ => false,
    }
}

fn build_ui(ui: &mut Ui, active: &mut usize) {
    let active_style = active_toolbar_button(&ui.theme.button);
    Panel::vstack()
        .auto_id()
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
                .auto_id()
                .gap(6.0)
                .line_gap(6.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    for (i, (label, _)) in SHOWCASES.iter().enumerate() {
                        let mut btn = Button::new().id_salt(*label).label(*label);
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
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .background(Background {
                    fill: Color::hex(0x343434).into(),
                    stroke: palantir::Stroke::solid(Color::hex(0x363636), 1.0),
                    radius: palantir::Corners::all(8.0),
                    shadow: Shadow::NONE,
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
        normal: default.hovered,
        ..default.clone()
    }
}
