//! The windowed host on Linux and the BSDs, under Wayland or X11.

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
