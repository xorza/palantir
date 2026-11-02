use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::error::GpuRequestError;
use crate::gpu::surface::surface_manager::{
    FirstWindow, HostGpuConfig, SurfaceManager, SurfaceStartup,
};
use crate::gpu::surface::window_surface::{Acquired, Prepared};
use crate::gpu::wgpu_backend::FrameTiming;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::platform;
use crate::host::winit::{WinitHost, finish_run};
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::sync::Arc;
use winit::error;
use winit::event_loop::EventLoop;
use winit::window::WindowAttributes;

/// An app that records nothing, to name the builder's type.
#[derive(Debug)]
struct Inert;

impl App for Inert {
    fn record(&mut self, _win: WindowToken, _ui: &mut Ui) {}
}

/// Each setter overrides its default.
#[test]
fn builder_setters_override_the_defaults() {
    let builder = WinitHost::<Inert>::builder(WindowToken(9))
        .window(WindowConfig::new("window"))
        .title("title")
        .power_preference(PowerPreference::HighPerformance)
        .collect_gpu_stats(true)
        .pixel_snap(false)
        .vsync(Vsync::Off)
        .fonts(FontScope::Bundled)
        .shaper(TextShaper::test_mono())
        .system_clipboard(false);
    let defaults = WinitHost::<Inert>::builder(WindowToken(3));

    assert_eq!(builder.first_token, WindowToken(9));
    assert_eq!(builder.config.window.title, "title");
    assert_eq!(
        (builder.config.vsync, defaults.config.vsync),
        (Vsync::Off, Vsync::On)
    );
    assert_eq!(
        (builder.config.options.fonts, defaults.config.options.fonts),
        (FontScope::Bundled, FontScope::System)
    );
    assert_eq!(
        (
            builder.config.power_preference,
            defaults.config.power_preference
        ),
        (PowerPreference::HighPerformance, PowerPreference::LowPower)
    );
    let (set, unset) = (&builder.config.options, &defaults.config.options);
    assert!(set.shaper.is_some() && unset.shaper.is_none());
    assert_eq!(
        (set.timing, unset.timing),
        (FrameTiming::On, FrameTiming::Off)
    );
    assert!(!set.pixel_snap && unset.pixel_snap);
    assert!(!set.system_clipboard && unset.system_clipboard);
}

#[test]
fn run_result_preserves_normal_exit_and_prioritizes_host_failure() {
    assert!(finish_run(None, Ok(())).is_ok());

    let loop_failure = finish_run(None, Err(error::EventLoopError::RecreationAttempt)).unwrap_err();
    assert!(matches!(
        loop_failure,
        WinitHostError::RunEventLoop {
            source: error::EventLoopError::RecreationAttempt
        }
    ));

    let host_failure = finish_run(
        Some(WinitHostError::Gpu {
            source: GpuRequestError::NoBackend,
        }),
        Err(error::EventLoopError::RecreationAttempt),
    )
    .unwrap_err();
    assert!(matches!(
        host_failure,
        WinitHostError::Gpu {
            source: GpuRequestError::NoBackend
        }
    ));
}

/// A new window's first configure, on a device already lost, builds no swapchain: it counts as
/// unbuilt and the acquire refuses, where wgpu's acquire from that surface panics ("Error in
/// Surface::get_current_texture_view: Validation Error"). On the device before the loss, another
/// window's first configure builds. Opens native windows, so it runs by hand:
/// `cargo drive test a_surface_configured_on_a_lost_device --ignored`.
#[test]
#[ignore = "opens native windows, which a headless machine has none of"]
fn a_surface_configured_on_a_lost_device_refuses_to_acquire() {
    let mut builder = EventLoop::builder();
    platform::internals::any_thread(&mut builder);
    let event_loop = builder.build().unwrap();
    let window = || {
        #[expect(
            deprecated,
            reason = "a test has no running loop to make its windows in, and they are all it needs"
        )]
        let window = event_loop.create_window(WindowAttributes::default().with_visible(false));
        Arc::new(window.unwrap())
    };
    let (lost_window, healthy_window) = (window(), window());
    let size = UVec2::splat(64);
    let SurfaceStartup {
        surfaces,
        gpu,
        first_surface: mut lost,
    } = SurfaceManager::start(
        FirstWindow {
            display: event_loop.owned_display_handle(),
            window: &lost_window,
            size,
        },
        HostGpuConfig::default(),
    )
    .unwrap();
    let mut healthy = surfaces.make_surface(&healthy_window, size).unwrap();
    assert_eq!(healthy.prepare(&gpu), Prepared::Rebuilt);

    gpu.lose();
    assert_eq!(lost.prepare(&gpu), Prepared::Kept);
    assert!(matches!(lost.acquire(), Acquired::Unconfigured));
}
