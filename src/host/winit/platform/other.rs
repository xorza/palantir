//! The windowed host on every target without its own file.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

use crate::theme::color_scheme::ColorScheme;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Nothing to apply.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// No scheme is assumed: `mundy`'s `NoPreference` reads as nothing.
pub(super) const NO_PREFERENCE: Option<ColorScheme> = None;

/// No double-click time is assumed: palantir's default holds.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;
