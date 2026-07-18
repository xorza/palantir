//! [`WinitHostConfig`] — startup tunables for [`WinitHost`](super::WinitHost).

use crate::window::WindowConfig;

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
    pub power_preference: wgpu::PowerPreference,
    /// Opt into GPU instrumentation (timestamp + pipeline-statistics
    /// queries). Off by default because the per-frame readback
    /// round-trip is non-trivial. Gates device-feature requests at
    /// startup; every window's `WindowDriver` inherits the result.
    pub collect_gpu_stats: bool,
}

impl Default for WinitHostConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            present_mode: wgpu::PresentMode::AutoVsync,
            power_preference: wgpu::PowerPreference::LowPower,
            collect_gpu_stats: false,
        }
    }
}
