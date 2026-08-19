//! The backend-agnostic options a window opens with.

use crate::primitives::image::Image;
use glam::{IVec2, UVec2};

/// Per-window options — what [`Ui::open_window`](crate::Ui::open_window)
/// takes (and what the first window's options live in inside
/// [`WinitHostConfig`](crate::WinitHostConfig)). Backend-agnostic by
/// design: no winit or wgpu types, so opening a window from app code
/// doesn't pull the windowing backend into the `Ui` API. Sizes are
/// `UVec2` logical pixels (DPI-independent), `.x` = width, `.y` = height
/// — the same integer-extent vocabulary as [`Display`](crate::Display).
#[derive(Clone, Debug, Default)]
pub struct WindowConfig {
    /// Native window title.
    pub title: String,
    /// Initial inner size in logical pixels. `None` lets the platform
    /// pick.
    pub inner_size: Option<UVec2>,
    /// Minimum inner size in logical pixels. `None` = no floor.
    pub min_inner_size: Option<UVec2>,
    /// Initial outer position in **physical** pixels (top-left of the
    /// window frame). `None` lets the platform place it. Physical, not
    /// logical, because a saved position is only unambiguous across
    /// mixed-DPI monitors in device pixels. The host drops it at creation
    /// if it no longer lands on any connected monitor, so a window saved
    /// on a since-disconnected display doesn't reopen off-screen.
    pub position: Option<IVec2>,
    /// Start maximized. Restored alongside `inner_size` — winit applies
    /// the maximized state and holds `inner_size` as the size to return to
    /// when the user un-maximizes.
    pub maximized: bool,
    /// Title-bar / taskbar icon. `None` = platform default. Honored on
    /// Windows and Linux (X11/Wayland); **macOS ignores per-window icons**
    /// (its Dock icon comes from the `.app` bundle's `.icns`, set at
    /// packaging time). [`WinitHost`](crate::WinitHost) converts the
    /// backend-agnostic [`Image`] to the platform icon at window creation.
    pub icon: Option<Image>,
    /// Application identity, as the desktop shell uses it to tie this window
    /// to its `.desktop` entry — Wayland's `app_id`, X11's `WM_CLASS`. Set it
    /// to the desktop file's basename (`org.example.App` for
    /// `org.example.App.desktop`).
    ///
    /// Worth setting even though it looks cosmetic: **Wayland has no
    /// fallback**. X11 derives `WM_CLASS` from `argv[0]` when nothing is
    /// given, but a Wayland window left unnamed has no `app_id` at all, and a
    /// shell with nothing to match on shows the window under a generic icon,
    /// detached from the launcher entry that started it.
    ///
    /// `None` keeps the platform default. Ignored on macOS and Windows, where
    /// application identity comes from the bundle or the executable.
    pub app_id: Option<String>,
}

impl WindowConfig {
    /// A config for a window titled `title`; every other option defaults
    /// (platform-picked size/position, not maximized, default icon). Chain
    /// the setters below to override.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Initial inner size in logical pixels (`.x` = width, `.y` = height).
    pub fn inner_size(mut self, size: UVec2) -> Self {
        self.inner_size = Some(size);
        self
    }

    /// Minimum inner size in logical pixels — the window can't shrink below
    /// it.
    pub fn min_inner_size(mut self, size: UVec2) -> Self {
        self.min_inner_size = Some(size);
        self
    }

    /// Initial outer position in physical pixels (top-left of the frame).
    /// Dropped at creation if it no longer lands on any connected monitor.
    pub fn position(mut self, position: IVec2) -> Self {
        self.position = Some(position);
        self
    }

    /// Start the window maximized (holding [`Self::inner_size`] as the
    /// un-maximize size).
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// Title-bar / taskbar icon (ignored on macOS).
    pub fn icon(mut self, icon: Image) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Desktop application identity — Wayland `app_id` / X11 `WM_CLASS`. Give
    /// it the `.desktop` entry's basename; see [`WindowConfig::app_id`] for
    /// why Wayland in particular needs it.
    pub fn app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::window::window_config::WindowConfig;
    use glam::{IVec2, UVec2};

    #[test]
    fn window_config_builders_populate_public_fields() {
        let config = WindowConfig::new("inspector")
            .inner_size(UVec2::new(800, 600))
            .min_inner_size(UVec2::new(320, 240))
            .position(IVec2::new(-40, 80))
            .maximized(true)
            .app_id("org.example.Inspector");

        assert_eq!(config.title, "inspector");
        assert_eq!(config.inner_size, Some(UVec2::new(800, 600)));
        assert_eq!(config.min_inner_size, Some(UVec2::new(320, 240)));
        assert_eq!(config.position, Some(IVec2::new(-40, 80)));
        assert!(config.maximized);
        assert!(config.icon.is_none());
        // Identity is distinct from the title: a shell matches the `.desktop`
        // entry on the id, so the two must not be conflated.
        assert_eq!(config.app_id.as_deref(), Some("org.example.Inspector"));
        assert!(WindowConfig::default().icon.is_none());
        // Unset by default — the platform's own default identity stands.
        assert!(WindowConfig::default().app_id.is_none());
        assert!(WindowConfig::new("inspector").app_id.is_none());
    }
}
