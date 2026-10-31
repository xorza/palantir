//! The windowed host on Linux and the BSDs, under Wayland or X11.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::wayland::WindowAttributesExtWayland as _;
use winit::window::WindowAttributes;

use crate::theme::color_scheme::ColorScheme;

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

/// The scheme a desktop that states no preference shows: light. The Settings portal's
/// `color-scheme` 0 is "No preference", and GNOME writes 0 for its own Light setting, which
/// libadwaita reads as light
/// (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Settings.html>).
/// `mundy` answers `NoPreference` for a desktop with no portal too, which then shows light.
pub(super) const NO_PREFERENCE: Option<ColorScheme> = Some(ColorScheme::Light);

/// GNOME's and KDE's default double-click time, 400 ms, kept where the portal gives none. GNOME's
/// setting reads through the portal's `org.gnome.desktop.peripherals.mouse double-click`; KDE keeps
/// its own in `kdeglobals`, which no portal key holds, so a KDE desktop answers the GSettings value
/// (`.notes/research/2026-10-31-system-preferences.md`).
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = Some(Duration::from_millis(400));
