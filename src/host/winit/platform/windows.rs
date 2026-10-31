//! The windowed host on Windows.

use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
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

/// The double-click time the person set in the Mouse settings, `GetDoubleClickTime`
/// (https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdoubleclicktime),
/// in milliseconds, at most 5000.
pub(super) fn double_click_window() -> Option<Duration> {
    // SAFETY: a read of a system setting, with no argument.
    let millis = unsafe { GetDoubleClickTime() };
    (millis > 0).then(|| Duration::from_millis(u64::from(millis)))
}

#[cfg(test)]
mod tests {
    use crate::host::winit::platform;
    use std::time::Duration;

    /// The system answers a positive time no longer than the 5000 ms Windows allows.
    #[test]
    fn the_system_answers_a_double_click_time() {
        let window = platform::double_click_window().expect("Windows sets a double-click time");
        assert!(
            window > Duration::ZERO && window <= Duration::from_millis(5000),
            "{window:?}"
        );
    }
}
