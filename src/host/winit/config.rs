//! [`WinitHostConfig`]: the startup settings a [`WinitHostBuilder`] collects for [`WinitHost`].
//!
//! [`WinitHostBuilder`]: super::WinitHostBuilder
//! [`WinitHost`]: super::WinitHost

use crate::gpu::device::power_preference::PowerPreference;
use crate::text::font_scope::FontScope;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;

/// The first window's [`WindowConfig`] plus the app-global GPU settings fixed at launch and shared by every window. Defaults and reasons are on the same-named [`WinitHostBuilder`](super::WinitHostBuilder) setters.
#[derive(Clone, Debug)]
pub(crate) struct WinitHostConfig {
    pub(crate) window: WindowConfig,
    pub(crate) vsync: Vsync,
    pub(crate) power_preference: PowerPreference,
    pub(crate) collect_gpu_stats: bool,
    pub(crate) fonts: FontScope,
    pub(crate) pixel_snap: bool,
}

impl Default for WinitHostConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            vsync: Vsync::On,
            power_preference: PowerPreference::LowPower,
            collect_gpu_stats: false,
            fonts: FontScope::System,
            pixel_snap: true,
        }
    }
}
