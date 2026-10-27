use crate::app::App;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::error::{GpuRequestError, SurfaceError};
use crate::host::winit::error::WinitHostError;
use crate::host::winit::{WinitHost, finish_run};
use crate::text::font_scope::FontScope;
use crate::ui::Ui;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use winit::error;

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
        .fonts(FontScope::Bundled);
    let defaults = WinitHost::<Inert>::builder(WindowToken(3));

    assert_eq!(builder.first_token, WindowToken(9));
    assert_eq!(builder.config.window.title, "title");
    assert_eq!(
        (builder.config.vsync, defaults.config.vsync),
        (Vsync::Off, Vsync::On)
    );
    assert_eq!(
        (builder.config.fonts, defaults.config.fonts),
        (FontScope::Bundled, FontScope::System)
    );
    assert_eq!(
        (
            builder.config.power_preference,
            defaults.config.power_preference
        ),
        (PowerPreference::HighPerformance, PowerPreference::LowPower)
    );
    assert!(builder.config.collect_gpu_stats && !defaults.config.collect_gpu_stats);
    assert!(!builder.config.pixel_snap && defaults.config.pixel_snap);
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
        Some(WinitHostError::Surface {
            token: WindowToken(0),
            source: SurfaceError::Device {
                source: GpuRequestError::NoBackend,
            },
        }),
        Err(error::EventLoopError::RecreationAttempt),
    )
    .unwrap_err();
    assert!(matches!(
        host_failure,
        WinitHostError::Surface {
            source: SurfaceError::Device {
                source: GpuRequestError::NoBackend
            },
            ..
        }
    ));
}
