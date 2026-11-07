//! The windowed host on every target without its own file.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

use crate::host::winit::input::wheel_steps::WheelSteps;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Nothing to apply.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// No double-click time is assumed: palantir's default holds.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;

/// One line per notch: no scroll setting is assumed.
pub(super) fn wheel_steps() -> WheelSteps {
    WheelSteps::default()
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;

    /// Nothing to allow: the platform's event loop takes no thread setting.
    pub(crate) const fn any_thread<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}
}
