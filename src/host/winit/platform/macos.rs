//! The windowed host on macOS.

use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::EventLoopBuilderExtMacOS;
use winit::window::WindowAttributes;

use crate::host::winit::input::wheel_steps::WheelSteps;

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

/// macOS always holds a double-click time, so none is kept here.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;

/// One line per notch: AppKit applies the person's scroll speed before the event arrives.
pub(super) fn wheel_steps() -> WheelSteps {
    WheelSteps::default()
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;

    /// Nothing to allow: AppKit runs an event loop on the main thread alone, so a test that
    /// builds one panics off it.
    pub(crate) const fn any_thread<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}
}
