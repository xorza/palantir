//! The windowed host on Windows.

use std::mem;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    POWERBROADCAST_SETTING, RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
};
use windows::Win32::System::SystemServices::GUID_POWER_SAVING_STATUS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, HWND_MESSAGE, MSG, PBT_POWERSETTINGCHANGE, PostMessageW, RegisterClassW,
    SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CLOSE, WM_POWERBROADCAST, WNDCLASSW,
};
use windows::core::{Error, GUID, PCWSTR, w};
use winit::event_loop::EventLoopBuilder;
use winit::window::WindowAttributes;

use crate::common::sync::MutexExt as _;
use crate::host::winit::input::wheel_steps::WheelSteps;
use crate::primitives::reduction::Reduction;

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

/// `GUID_ENERGY_SAVER_STATUS` from WinNT.h, which `windows` 0.62 does not declare. Microsoft's
/// table of power setting GUIDs gives it, and Windows 11 25H2 answers a registration for it
/// (<https://learn.microsoft.com/windows/win32/power/power-setting-guids>).
const GUID_ENERGY_SAVER_STATUS: GUID = GUID::from_u128(0x550e8400_e29b_41d4_a716_446655440000);

/// `ENERGY_SAVER_OFF` of `ENERGY_SAVER_STATUS`, beside `ENERGY_SAVER_STANDARD` (1) and
/// `ENERGY_SAVER_HIGH_SAVINGS` (2).
const ENERGY_SAVER_OFF: u32 = 0;

/// The message the watch's window posts to itself with a saver setting's new value: the value in
/// `wParam`, the setting in `lParam`.
const WM_SAVER_SETTING: u32 = WM_APP;

/// The watch's window class, registered once a process.
const WINDOW_CLASS: PCWSTR = w!("PalantirPowerSaverWatch");

/// Follows the saver mode through two power settings: `GUID_ENERGY_SAVER_STATUS`, Energy saver on
/// Windows 11 24H2 and later, and `GUID_POWER_SAVING_STATUS`, battery saver before it, the route
/// Microsoft gives for battery saver
/// (<https://learn.microsoft.com/windows/win32/power/what-s-new-in-power-management>). Power is
/// reduced while either says so, so one watch serves every version with no version check.
///
/// Both are registered to a message-only window on a thread of its own, as Microsoft asks of an
/// interactive app and as Chromium's power monitor does, and the thread's message loop holds the
/// settings' values
/// (<https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerpowersettingnotification>).
/// The watch's drop closes the window, whose thread unregisters both before it ends, so no
/// notification outlives what it reads. Refused: `PowerSettingRegisterNotification`'s callback,
/// whose context no documented rule keeps from a callback still running after the unregister;
/// `PowerRegisterForEffectivePowerModeNotifications`, whose modes mix the saver with the
/// performance slider.
#[derive(Debug)]
pub(super) struct PowerSaverWatch {
    window: Arc<Mutex<WatchWindow>>,
    /// The window's thread; `None` where no thread started, which leaves power full.
    thread: Option<JoinHandle<()>>,
}

/// The watch's window, as its thread opens it and the watch's drop closes it.
#[derive(Debug)]
enum WatchWindow {
    Opening,
    Open(WindowHandle),
    Closed,
}

/// The watch's window handle, which the watch's drop posts its close to from another thread.
#[derive(Debug)]
struct WindowHandle(HWND);

// SAFETY: a window handle names the window to any thread of the process, and the watch only posts
// to it, which Windows allows from any thread.
unsafe impl Send for WindowHandle {}

/// Which saver setting a power setting notification names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaverSetting {
    EnergySaver,
    BatterySaver,
}

impl SaverSetting {
    const ALL: [Self; 2] = [Self::EnergySaver, Self::BatterySaver];

    const fn guid(self) -> GUID {
        match self {
            Self::EnergySaver => GUID_ENERGY_SAVER_STATUS,
            Self::BatterySaver => GUID_POWER_SAVING_STATUS,
        }
    }

    fn of(guid: GUID) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.guid() == guid)
    }
}

/// The last value of each saver setting; 0, off, until its first notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SaverStatuses {
    /// An `ENERGY_SAVER_STATUS`.
    energy_saver: u32,
    /// 0 for battery saver off, 1 for on.
    battery_saver: u32,
}

impl SaverStatuses {
    /// These statuses with `setting` at `value`.
    const fn with(self, setting: SaverSetting, value: u32) -> Self {
        match setting {
            SaverSetting::EnergySaver => Self {
                energy_saver: value,
                ..self
            },
            SaverSetting::BatterySaver => Self {
                battery_saver: value,
                ..self
            },
        }
    }

    /// Reduced while either saver is on, at any level: Energy saver's standard and high-savings
    /// levels both reduce, as palantir has one reduced level.
    const fn power(self) -> Reduction {
        Reduction::of(self.energy_saver != ENERGY_SAVER_OFF || self.battery_saver != 0)
    }
}

impl PowerSaverWatch {
    /// Follow the saver mode: `on_change` runs on the watch's thread with the mode as the
    /// registrations answer, which Windows does at once, then with each change.
    pub(super) fn follow(on_change: impl Fn(Reduction) + Send + Sync + 'static) -> Self {
        let window = Arc::new(Mutex::new(WatchWindow::Opening));
        let held = Arc::clone(&window);
        let thread = thread::Builder::new()
            .name("palantir-power-saver".to_owned())
            .spawn(move || {
                if let Err(error) = follow_settings(&held, &on_change) {
                    tracing::debug!(%error, "the saver mode is not followed");
                }
            })
            .inspect_err(|error| tracing::warn!(%error, "no thread follows the saver mode"))
            .ok();
        Self { window, thread }
    }
}

impl Drop for PowerSaverWatch {
    fn drop(&mut self) {
        let held = mem::replace(&mut *self.window.lock_unpoisoned(), WatchWindow::Closed);
        if let WatchWindow::Open(WindowHandle(window)) = held {
            // SAFETY: the window lives until its thread reads this message.
            unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.ok();
        }
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// Open the window, register both settings to it, and report each value until the watch closes
/// the window.
fn follow_settings(held: &Mutex<WatchWindow>, on_change: &impl Fn(Reduction)) -> Result<(), Error> {
    let window = open_window()?;
    {
        let mut held = held.lock_unpoisoned();
        if matches!(*held, WatchWindow::Closed) {
            // SAFETY: this thread made the window.
            return unsafe { DestroyWindow(window) };
        }
        *held = WatchWindow::Open(WindowHandle(window));
    }
    let registrations = SaverSetting::ALL.map(|setting| {
        let guid = setting.guid();
        // SAFETY: the window lives until both registrations end below.
        unsafe {
            RegisterPowerSettingNotification(
                HANDLE(window.0),
                &raw const guid,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        }
        .inspect_err(|error| tracing::debug!(?setting, %error, "a saver setting is not followed"))
        .ok()
    });
    let mut statuses = SaverStatuses::default();
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a valid out-parameter; the loop reads this thread's messages.
        match unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0 {
            0 => break,
            -1 => {
                tracing::debug!(error = %Error::from_thread(), "the saver window's loop failed");
                break;
            }
            _ => {}
        }
        match message.message {
            WM_SAVER_SETTING => {
                let setting = SaverSetting::ALL[usize::try_from(message.lParam.0)
                    .expect("the window posts a setting's index")];
                let value =
                    u32::try_from(message.wParam.0).expect("the window posts a setting's DWORD");
                statuses = statuses.with(setting, value);
                on_change(statuses.power());
            }
            WM_CLOSE => break,
            // SAFETY: `message` came from `GetMessageW`.
            _ => unsafe {
                DispatchMessageW(&raw const message);
            },
        }
    }
    for registration in registrations.into_iter().flatten() {
        // SAFETY: the handle came from `RegisterPowerSettingNotification`, unregistered once.
        unsafe { UnregisterPowerSettingNotification(registration) }.ok();
    }
    // SAFETY: this thread made the window.
    unsafe { DestroyWindow(window) }
}

/// A message-only window of [`WINDOW_CLASS`], registering the class on first use.
fn open_window() -> Result<HWND, Error> {
    // SAFETY: `None` names the executable's own module.
    let instance = HINSTANCE::from(unsafe { GetModuleHandleW(None) }?);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: WINDOW_CLASS,
        ..WNDCLASSW::default()
    };
    // SAFETY: `class` names a window procedure and a class name that live for the process.
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        let error = Error::from_thread();
        if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    // SAFETY: the class is registered above.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            PCWSTR::null(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
    }
}

/// The window's procedure: a saver setting's notification is posted back to the window, whose
/// message loop holds the values; every other message is the default's.
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_POWERBROADCAST && wparam.0 == PBT_POWERSETTINGCHANGE as usize {
        let setting = lparam.0 as *const POWERBROADCAST_SETTING;
        // SAFETY: Windows hands a `POWERBROADCAST_SETTING` whose `Data` holds `DataLength` bytes,
        // read through the raw pointer, as the struct declares one byte of them; bytes need no
        // alignment.
        let notified = unsafe {
            ((*setting).DataLength == 4).then(|| {
                let data = (&raw const (*setting).Data).cast::<[u8; 4]>();
                ((*setting).PowerSetting, u32::from_ne_bytes(data.read()))
            })
        };
        if let Some((guid, value)) = notified
            && let Some(setting) = SaverSetting::of(guid)
        {
            // SAFETY: the window is this procedure's own.
            unsafe {
                PostMessageW(
                    Some(window),
                    WM_SAVER_SETTING,
                    WPARAM(usize::try_from(value).expect("a DWORD fits a WPARAM")),
                    LPARAM(setting as isize),
                )
            }
            .ok();
        }
        return LRESULT(1);
    }
    // SAFETY: the arguments are the procedure's own.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
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
    use windows::Win32::System::SystemServices::GUID_POWER_SAVING_STATUS;
    use windows::core::GUID;

    use crate::host::winit::platform::{
        GUID_ENERGY_SAVER_STATUS, SaverSetting, SaverStatuses, WHEEL_PAGESCROLL, steps_of,
    };
    use crate::primitives::reduction::Reduction;

    /// Each setting's notification sets its own status and keeps the other's: Energy saver off,
    /// standard (1) and high savings (2), with battery saver off and on. Power is full only while
    /// both are off. A GUID that is neither names no setting.
    #[test]
    fn either_saver_on_at_any_level_reduces_power() {
        assert_eq!(
            SaverSetting::of(GUID_ENERGY_SAVER_STATUS),
            Some(SaverSetting::EnergySaver)
        );
        assert_eq!(
            SaverSetting::of(GUID_POWER_SAVING_STATUS),
            Some(SaverSetting::BatterySaver)
        );
        assert_eq!(SaverSetting::of(GUID::zeroed()), None);
        for (energy_saver, battery_saver, power) in [
            (0, 0, Reduction::Full),
            (1, 0, Reduction::Reduced),
            (2, 0, Reduction::Reduced),
            (0, 1, Reduction::Reduced),
            (2, 1, Reduction::Reduced),
        ] {
            let statuses = SaverStatuses::default()
                .with(SaverSetting::BatterySaver, battery_saver)
                .with(SaverSetting::EnergySaver, energy_saver);
            assert_eq!(
                statuses,
                SaverStatuses {
                    energy_saver,
                    battery_saver,
                },
                "each setting keeps the other's"
            );
            assert_eq!(
                statuses.power(),
                power,
                "energy saver {energy_saver}, battery saver {battery_saver}"
            );
        }
    }

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
