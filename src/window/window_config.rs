//! The backend-agnostic options a window opens with.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::paint::image::Image;
use crate::window::window_geometry::WindowGeometry;
use crate::window::window_placement::WindowPlacement;
use glam::IVec2;

/// Per-window options for [`Ui::open_window`](crate::Ui::open_window) and the
/// first window of [`WinitHostBuilder`](crate::WinitHostBuilder). Backend-agnostic; sizes are
/// [`Size`] in logical pixels, as every logical length is, and a position is physical.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct WindowConfig {
    /// Native window title.
    pub title: String,
    /// Initial inner size in logical pixels; `None` lets the platform pick.
    pub inner_size: Option<Size>,
    /// Minimum inner size in logical pixels; `None` = no floor.
    pub min_inner_size: Option<Size>,
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
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [positive](crate::widget::domain::is_positive).
    #[track_caller]
    pub const fn with_inner_size(mut self, size: Size) -> Self {
        assert_window_size(size);
        self.inner_size = Some(size);
        self
    }

    /// Minimum inner size in logical pixels.
    ///
    /// # Panics
    ///
    /// As [`Self::with_inner_size`].
    #[track_caller]
    pub const fn with_min_inner_size(mut self, size: Size) -> Self {
        assert_window_size(size);
        self.min_inner_size = Some(size);
        self
    }

    /// Every size this config holds, checked where the config enters a host, as its fields are
    /// plain data: [`Ui::open_window`](crate::Ui::open_window) and the bootstrap window's builder.
    ///
    /// # Panics
    ///
    /// As [`Self::with_inner_size`].
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if let Some(size) = self.inner_size {
            assert_window_size(size);
        }
        if let Some(size) = self.min_inner_size {
            assert_window_size(size);
        }
    }

    /// Initial outer position in physical pixels.
    pub const fn with_physical_pos(mut self, pos: IVec2) -> Self {
        self.placement.physical_pos = Some(pos);
        self
    }

    /// The size, position and maximized state a persisted [`WindowGeometry`] holds, to open the
    /// window where it was.
    ///
    /// # Panics
    ///
    /// As [`Self::with_inner_size`].
    #[track_caller]
    pub const fn with_geometry(mut self, geometry: WindowGeometry) -> Self {
        assert_window_size(geometry.inner_size);
        self.inner_size = Some(geometry.inner_size);
        self.placement = geometry.placement;
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

    /// Desktop application identity; see [`Self::app_id`].
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }
}

/// A window's size: both axes positive, as a window of no extent the platform refuses and a
/// boundless one it cannot make.
#[track_caller]
const fn assert_window_size(size: Size) {
    assert!(
        domain::is_positive(size.w) && domain::is_positive(size.h),
        "a window's size must be positive on both axes"
    );
}

#[cfg(test)]
mod tests {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::size::Size;
    use crate::window::window_config::WindowConfig;
    use crate::window::window_placement::WindowPlacement;
    use glam::IVec2;

    #[test]
    fn window_config_builders_populate_public_fields() {
        let config = WindowConfig::new("inspector")
            .with_inner_size(Size::new(800.0, 600.0))
            .with_min_inner_size(Size::new(320.0, 240.0))
            .with_physical_pos(IVec2::new(-40, 80))
            .with_maximized(true)
            .with_app_id("org.example.Inspector");

        assert_eq!(config.title, "inspector");
        assert_eq!(config.inner_size, Some(Size::new(800.0, 600.0)));
        assert_eq!(config.min_inner_size, Some(Size::new(320.0, 240.0)));
        assert_eq!(
            config.placement,
            WindowPlacement {
                physical_pos: Some(IVec2::new(-40, 80)),
                maximized: true,
            },
        );
        assert!(config.icon.is_none());
        assert_eq!(config.app_id.as_deref(), Some("org.example.Inspector"));
        assert!(WindowConfig::default().icon.is_none());
        assert!(WindowConfig::default().app_id.is_none());
        assert!(WindowConfig::new("inspector").app_id.is_none());

        for size in [
            Size::new(0.0, 600.0),
            Size::new(800.0, f32::INFINITY),
            Size::new(f32::NAN, 600.0),
        ] {
            panic_probe::assert_panics_with("a window's size must be positive", || {
                WindowConfig::default().with_inner_size(size)
            });
        }
    }
}
