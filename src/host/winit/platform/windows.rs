//! The windowed host on Windows.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Nothing to apply: on Windows the application's identity comes from its executable.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// Windows always holds a double-click time, so none is kept here.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;
    use winit::platform::windows::EventLoopBuilderExtWindows as _;

    /// Let a test, which runs off the main thread, build an event loop.
    pub(crate) fn any_thread<T: 'static>(builder: &mut EventLoopBuilder<T>) {
        builder.with_any_thread(true);
    }
}
