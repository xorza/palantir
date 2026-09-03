//! [`WinitHostConfig`] — startup tunables for [`WinitHost`](super::WinitHost).

use crate::text::font_scope::FontScope;
use crate::window::window_config::WindowConfig;

/// Startup tunables for [`WinitHost`](super::WinitHost): the first
/// window's [`WindowConfig`] plus the **app-global** GPU knobs that are
/// fixed once at launch and shared by every window — the adapter power
/// preference, the swapchain present mode, and the GPU-instrumentation
/// opt-in. Secondary windows ([`Ui::open_window`](crate::Ui::open_window))
/// only carry a [`WindowConfig`]; they inherit these.
#[derive(Clone, Debug)]
pub struct WinitHostConfig {
    /// The first window's options.
    pub window: WindowConfig,
    /// App-global presentation policy requested for every window. Supported
    /// explicit modes are kept; unsupported ones use the matching automatic
    /// policy for that surface.
    pub present_mode: wgpu::PresentMode,
    /// Adapter power preference — selects the shared adapter at startup.
    ///
    /// `LowPower` by default, unlike the headless paths, which ask for
    /// `HighPerformance`. The difference is deliberate: a window is a user
    /// interface and on a hybrid laptop the integrated GPU draws it
    /// without waking the discrete one, while a bench or a golden test is
    /// worth little unless it runs on the adapter a user is looking at.
    /// An application that draws something heavier should say so here.
    pub power_preference: wgpu::PowerPreference,
    /// Opt into GPU instrumentation (timestamp + pipeline-statistics
    /// queries). Off by default because the per-frame readback
    /// round-trip is non-trivial. Gates device-feature requests at
    /// startup; every window's `WindowDriver` inherits the result.
    pub collect_gpu_stats: bool,
    /// Which faces the app-global shaper starts with.
    ///
    /// [`FontScope::System`] by default, unlike
    /// [`TextShaper::new`](crate::TextShaper::new): a window is what a
    /// person reads, and the OS fonts are the glyph fallback that keeps
    /// scripts the bundled faces do not cover from rendering as tofu. The
    /// scan runs on its own thread beside GPU init, so it costs no wall
    /// time on a warm disk cache.
    pub fonts: FontScope,
    /// Whether axis-aligned paint edges snap to physical pixels. On by
    /// default, which is what a window wants: an unsnapped edge lands
    /// between texels and antialiases into a soft line. Turn it off for
    /// a view that animates position continuously, where the snap reads
    /// as a stutter.
    pub pixel_snap: bool,
}

impl Default for WinitHostConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            present_mode: wgpu::PresentMode::AutoVsync,
            power_preference: wgpu::PowerPreference::LowPower,
            collect_gpu_stats: false,
            fonts: FontScope::System,
            pixel_snap: true,
        }
    }
}
