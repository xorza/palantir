//! The windowed host on macOS.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::EventLoopBuilderExtMacOS;
use winit::window::WindowAttributes;

use crate::theme::color_scheme::ColorScheme;

/// Drops winit's default menu, which binds ⌘Q to `terminate:` and kills the process before the app
/// can veto `CloseRequested`; ⌘Q then arrives as an ordinary key event.
pub(super) fn configure_event_loop<T: 'static>(builder: &mut EventLoopBuilder<T>) {
    builder.with_default_menu(false);
}

/// Nothing to apply: the application's identity comes from its `.app` bundle.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// macOS always states light or dark, so `mundy`'s `NoPreference` is a failed read: nothing.
pub(super) const NO_PREFERENCE: Option<ColorScheme> = None;

/// macOS always holds a double-click time, so none is kept here.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;
