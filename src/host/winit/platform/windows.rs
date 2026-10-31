//! The windowed host on Windows.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

use crate::theme::color_scheme::ColorScheme;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Nothing to apply: on Windows the application's identity comes from its executable.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// Windows always states light or dark apps, so `mundy`'s `NoPreference` is a failed read:
/// nothing.
pub(super) const NO_PREFERENCE: Option<ColorScheme> = None;

/// Windows always holds a double-click time, so none is kept here.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;
