//! [`WinitHostConfig`]: the startup settings a [`WinitHostBuilder`] collects for [`WinitHost`].
//!
//! [`WinitHostBuilder`]: super::WinitHostBuilder
//! [`WinitHost`]: super::WinitHost

use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::surface::surface_manager::HostGpuConfig;
use crate::host::host_options::HostOptions;
use crate::text::font_scope::FontScope;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;

/// The first window's [`WindowConfig`] plus the app-global GPU settings fixed at launch and shared by every window. Defaults and reasons are on the same-named [`WinitHostBuilder`](super::WinitHostBuilder) setters.
#[derive(Debug)]
pub(super) struct WinitHostConfig {
    pub(super) window: WindowConfig,
    pub(super) vsync: Vsync,
    pub(super) power_preference: PowerPreference,
    pub(super) options: HostOptions,
}

impl Default for WinitHostConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            vsync: Vsync::On,
            power_preference: PowerPreference::LowPower,
            options: HostOptions {
                system_clipboard: true,
                ..HostOptions::new(FontScope::System)
            },
        }
    }
}

impl WinitHostConfig {
    /// The device and swapchain half, which the surface manager opens with.
    pub(super) const fn gpu_config(&self) -> HostGpuConfig {
        HostGpuConfig {
            power_preference: self.power_preference,
            vsync: self.vsync,
            timing: self.options.timing,
        }
    }
}
