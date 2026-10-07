//! The backend-agnostic options a window opens with.

use crate::primitives::paint::image::Image;
use crate::window::window_placement::WindowPlacement;
use glam::{IVec2, UVec2};

/// Per-window options for [`Ui::open_window`](crate::Ui::open_window) and the
/// first window of [`WinitHostBuilder`](crate::WinitHostBuilder). Backend-agnostic;
/// sizes are `UVec2` logical pixels, `.x` = width.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct WindowConfig {
    /// Native window title.
    pub title: String,
    /// Initial inner size in logical pixels; `None` lets the platform pick.
    pub inner_size: Option<UVec2>,
    /// Minimum inner size in logical pixels; `None` = no floor.
    pub min_inner_size: Option<UVec2>,
    /// Where to open the window, as [`WindowGeometry::placement`](crate::WindowGeometry)
    /// returns it. The host drops a position no longer on a connected monitor.
    pub placement: WindowPlacement,
    /// Title-bar and taskbar icon; `None` = platform default. Ignored on macOS,
    /// whose Dock icon comes from the bundle.
    pub icon: Option<Image>,
    /// Application identity the desktop shell ties to the window's `.desktop`
    /// entry: Wayland `app_id`, X11 `WM_CLASS`. Set it to the desktop file's
    /// basename (`org.example.App`); Wayland has no fallback and shows a generic
    /// icon otherwise. `None` keeps the platform default; ignored on macOS and Windows.
    pub app_id: Option<String>,
}

impl WindowConfig {
    /// A config for a window titled `title`; every other option defaults.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Initial inner size in logical pixels.
    pub const fn with_inner_size(mut self, size: UVec2) -> Self {
        self.inner_size = Some(size);
        self
    }

    /// Minimum inner size in logical pixels.
    pub const fn with_min_inner_size(mut self, size: UVec2) -> Self {
        self.min_inner_size = Some(size);
        self
    }

    /// Initial outer position in physical pixels.
    pub const fn with_position(mut self, position: IVec2) -> Self {
        self.placement.position = Some(position);
        self
    }

    /// Position and maximized state together, for restoring a persisted [`WindowGeometry::placement`](crate::WindowGeometry).
    pub const fn with_placement(mut self, placement: WindowPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Starts maximized; [`Self::with_inner_size`] is the un-maximize size.
    pub const fn with_maximized(mut self, maximized: bool) -> Self {
        self.placement.maximized = maximized;
        self
    }

    /// Title-bar and taskbar icon (ignored on macOS).
    pub fn with_icon(mut self, icon: Image) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Desktop application identity; see [`WindowConfig::with_app_id`].
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::window::window_config::WindowConfig;
    use crate::window::window_placement::WindowPlacement;
    use glam::{IVec2, UVec2};

    #[test]
    fn window_config_builders_populate_public_fields() {
        let config = WindowConfig::new("inspector")
            .with_inner_size(UVec2::new(800, 600))
            .with_min_inner_size(UVec2::new(320, 240))
            .with_position(IVec2::new(-40, 80))
            .with_maximized(true)
            .with_app_id("org.example.Inspector");

        assert_eq!(config.title, "inspector");
        assert_eq!(config.inner_size, Some(UVec2::new(800, 600)));
        assert_eq!(config.min_inner_size, Some(UVec2::new(320, 240)));
        assert_eq!(
            config.placement,
            WindowPlacement {
                position: Some(IVec2::new(-40, 80)),
                maximized: true,
            },
        );
        assert!(config.icon.is_none());
        assert_eq!(config.app_id.as_deref(), Some("org.example.Inspector"));
        assert!(WindowConfig::default().icon.is_none());
        assert!(WindowConfig::default().app_id.is_none());
        assert!(WindowConfig::new("inspector").app_id.is_none());
    }
}
