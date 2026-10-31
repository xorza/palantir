//! The windowed host on macOS.

use std::time::Duration;

use objc2_app_kit::NSEvent;
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::EventLoopBuilderExtMacOS;
use winit::window::WindowAttributes;

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

/// The double-click time the person set in System Settings, `NSEvent.doubleClickInterval`
/// (https://developer.apple.com/documentation/appkit/nsevent/doubleclickinterval), in seconds; a
/// NaN, negative, zero or overflowing value keeps palantir's default.
pub(super) fn double_click_window() -> Option<Duration> {
    // SAFETY: a class property that reads the user's defaults, with no argument and no thread
    // requirement.
    let seconds = unsafe { NSEvent::doubleClickInterval() };
    Duration::try_from_secs_f64(seconds)
        .ok()
        .filter(|window| !window.is_zero())
}

#[cfg(test)]
mod tests {
    use crate::host::winit::platform;
    use std::time::Duration;

    /// The system answers a positive time no longer than the five seconds the Mouse settings'
    /// slowest step allows.
    #[test]
    fn the_system_answers_a_double_click_time() {
        let window = platform::double_click_window().expect("macOS sets a double-click time");
        assert!(
            window > Duration::ZERO && window <= Duration::from_secs(5),
            "{window:?}"
        );
    }
}
