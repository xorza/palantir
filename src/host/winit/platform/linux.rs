//! The windowed host on Linux and the BSDs, under Wayland or X11.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::wayland::WindowAttributesExtWayland as _;
use winit::window::WindowAttributes;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Applies [`WindowConfig::with_app_id`](crate::WindowConfig::with_app_id). Wayland's `app_id` and
/// X11's `WM_CLASS` are one winit attribute; the instance name repeats the general one. Wayland has
/// no fallback, so an unnamed window cannot match its `.desktop` entry.
pub(super) fn with_app_id(attrs: WindowAttributes, app_id: Option<&str>) -> WindowAttributes {
    match app_id {
        Some(app_id) => attrs.with_name(app_id, app_id),
        None => attrs,
    }
}

/// GNOME's and KDE's default double-click time, 400 ms, not the person's setting. Under Wayland no
/// cross-desktop API holds it: GNOME keeps it in GSettings
/// (`org.gnome.desktop.peripherals.mouse double-click`), KDE in `kdeglobals`, and the Settings
/// portal's GNOME and GTK backends fail to read the key
/// (https://discourse.gnome.org/t/what-should-i-use-to-get-the-double-click-time/19917/2).
/// Under X11, XSETTINGS publishes it as `Net/DoubleClickTime`
/// (https://wiki.freedesktop.org/www/Specifications/XSettingsRegistry/), which this host does not
/// read. Refused: a D-Bus client for a read that fails, and a spawned `gsettings`, GNOME alone.
#[expect(
    clippy::unnecessary_wraps,
    reason = "one signature across the platform files, and a target with no setting answers none"
)]
pub(super) const fn double_click_window() -> Option<Duration> {
    Some(Duration::from_millis(400))
}

#[cfg(test)]
mod tests {
    use crate::host::winit::platform;
    use std::time::Duration;

    /// The desktops' default.
    #[test]
    fn the_double_click_time_is_the_desktops_default() {
        assert_eq!(
            platform::double_click_window(),
            Some(Duration::from_millis(400))
        );
    }
}
