use crate::app::App;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::error::GpuRequestError;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::{WinitHost, finish_run};
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
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
    assert!(set.collect_gpu_stats && !unset.collect_gpu_stats);
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
