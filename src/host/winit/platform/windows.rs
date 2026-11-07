//! The windowed host on Windows.

use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

use crate::host::winit::input::wheel_steps::WheelSteps;

/// `WHEEL_PAGESCROLL`: the lines-per-notch value that means a page per notch. The value is the
/// Win32 headers'; `windows` does not declare it.
const WHEEL_PAGESCROLL: u32 = u32::MAX;

/// What the two settings hold on a stock system, kept where a read fails, as Microsoft's own wheel
/// sample keeps it (<https://devblogs.microsoft.com/oldnewthing/20030807-00/?p=42963>).
const STOCK_WHEEL_SETTING: u32 = 3;

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

/// The person's scroll settings, read at each wheel event, as Microsoft's sample reads them, since
/// winit reports no change of them. `SPI_GETWHEELSCROLLCHARS` is the horizontal wheel's setting, in
/// characters, which a lane counts as lines, as no character width is known here. Cost: a sideways
/// step is wider than the setting's characters.
pub(super) fn wheel_steps() -> WheelSteps {
    steps_of(
        wheel_setting(SPI_GETWHEELSCROLLLINES),
        wheel_setting(SPI_GETWHEELSCROLLCHARS),
    )
}

fn wheel_setting(setting: SYSTEM_PARAMETERS_INFO_ACTION) -> u32 {
    let mut value = STOCK_WHEEL_SETTING;
    let read = unsafe {
        SystemParametersInfoW(
            setting,
            0,
            Some((&raw mut value).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if read.is_ok() {
        value
    } else {
        STOCK_WHEEL_SETTING
    }
}

const fn steps_of(lines: u32, chars: u32) -> WheelSteps {
    WheelSteps {
        vertical_lines: if lines == WHEEL_PAGESCROLL {
            None
        } else {
            Some(lines as f32)
        },
        horizontal_lines: chars as f32,
    }
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;
    use winit::platform::windows::EventLoopBuilderExtWindows as _;

    /// Let a test, which runs off the main thread, build an event loop.
    pub(crate) fn any_thread<T: 'static>(builder: &mut EventLoopBuilder<T>) {
        builder.with_any_thread(true);
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::input::wheel_steps::WheelSteps;
    use crate::host::winit::platform::{WHEEL_PAGESCROLL, steps_of};

    /// The stock three lines, a setting of none, and a page per notch, which only the vertical
    /// setting can state.
    #[test]
    fn each_setting_gives_its_step() {
        let rows = [
            (3, 3, Some(3.0), 3.0),
            (0, 5, Some(0.0), 5.0),
            (WHEEL_PAGESCROLL, 1, None, 1.0),
        ];
        for (lines, chars, vertical_lines, horizontal_lines) in rows {
            assert_eq!(
                steps_of(lines, chars),
                WheelSteps {
                    vertical_lines,
                    horizontal_lines,
                },
                "{lines} lines, {chars} chars",
            );
        }
    }
}
