# System preferences: color scheme and double-click time (2026-10-31)

Facts behind `host/winit/system_preferences.rs`, which reads them through `mundy`.

## winit 0.30.13

- `Window::theme`: "iOS / Android / x11 / Orbital: Unsupported", "Wayland: Only returns theme
  overrides". `WindowEvent::ThemeChanged`: "iOS / Android / X11 / Wayland / Orbital:
  Unsupported" (`src/window.rs`, `src/event.rs`).
- macOS: the window delegate observes the window's `effectiveAppearance` and returns early,
  sending no `ThemeChanged`, while `window.appearance()` is set; `theme()` returns that pinned
  appearance before it asks `NSApplication` (`src/platform_impl/macos/window_delegate.rs`,
  lines 461–466 and 1675–1688). So `set_theme(Some(_))` hides every system change.

## The XDG Settings portal

- `org.freedesktop.appearance color-scheme`: 0 "No preference", 1 "Prefer dark appearance", 2
  "Prefer light appearance"; unknown values read as 0. `SettingChanged(namespace, key, value)`
  signals a change
  (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Settings.html>).
- GNOME writes 0 for its Light setting; libadwaita's default scheme reads 0 as light.
- Measured on a Linux desktop (two portal backends), with `busctl --user
  call … org.freedesktop.portal.Settings ReadOne`:

  | Namespace, key | Answer |
  |---|---|
  | `org.freedesktop.appearance color-scheme` | `u 1` |
  | `org.freedesktop.appearance reduced-motion` | `u 1` |
  | `org.freedesktop.appearance contrast` | `u 0` |
  | `org.freedesktop.appearance accent-color` | `(ddd) 0.584 0.733 0.090` |
  | `org.gnome.desktop.peripherals.mouse double-click` | `i 400` |
  | `org.kde.kdeglobals.KDE DoubleClickInterval` | not found |

  The GNOME double-click key reads through the portal; on KDE it answers the GSettings value,
  not KDE's own setting.

## Crates (crates.io and GitHub, 2026-10-31)

| Crate | Version, date | Reads | Change source | Users |
|---|---|---|---|---|
| `mundy` | 0.2.3, 2026-04-30; Apache-2.0 | scheme, contrast, reduced motion and transparency, accent, double-click time, scrollbar visibility | Linux portal signal; macOS KVO; Windows `UISettings` | `iced_winit`, `freya-winit`, `neovide` |
| `dark-light` | 3.0.0, 2026-08-12 | scheme | Linux portal; macOS polls once a second | — |
| `system-theme` | 0.3.0, 2026-01-21 | scheme | — | 16 stars, 4.5k downloads |
| `ashpd` | 0.13.13 | every portal, Linux only | async | `rfd` and others |

- `mundy` maps the portal's 0, unknown values and read errors to `NoPreference`, and errors
  are silent unless its `log` feature is on (`src/freedesktop/mod.rs`). It reads the GNOME
  double-click key through the portal.
- `mundy` with `async-io`, `callback`, `color-scheme`, `double-click-interval`, counted by
  `cargo tree` against palantir's `Cargo.lock`: Linux 36 crates new (the `zbus` 5 and
  `async-*` family), macOS 9, Windows 7. `windows` 0.62 and `objc2-app-kit` 0.3 are in the lock
  already.
- `iced_winit` 0.14 takes the scheme from winit on macOS and Windows and from `mundy` on Linux,
  behind its optional `linux-theme-detection` feature.

## `mundy` in the winit host (measured 2026-10-31, a Linux desktop)

- `Preferences::once_blocking(ColorScheme | DoubleClickInterval)` through the portal: 20.3 ms
  for the first read of a process (the D-Bus connection), then 1.0–1.4 ms; answers `Dark` and
  400 ms on that desktop.
- A clean release build of `mundy` with palantir's features, alone in a scratch crate: 16.7 s on
  16 threads. palantir's `Cargo.lock` gains 46 package entries across the three targets.
- `Preferences::subscribe` runs its future on a thread `mundy` spawns ("mundy subscription
  thread", `src/async_rt.rs`) on every non-wasm target, so the callback is never on the main
  thread; `once_blocking` and `subscribe` are called on the main thread, as `mundy`'s caveats
  ask for macOS.

## Double-click time and distance elsewhere

- Windows: `GetDoubleClickTime`, in ms, at most 5000, default 500
  (<https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getdoubleclicktime>).
- macOS: `NSEvent.doubleClickInterval`.
- Linux: no cross-desktop standard. XSETTINGS `Net/DoubleClickTime` (default 250 ms) and
  `Net/DoubleClickDistance` (5 px), read by GTK; on Wayland GTK reads
  `org.gnome.desktop.peripherals.mouse double-click` through the Settings portal
  (<https://wiki.freedesktop.org/www/Specifications/XSettingsRegistry/>).
- egui: `InputOptions::{max_double_click_delay, max_click_dist}`, 0.3 s by default, set by the
  app.

## Power saving (2026-11-08)

### What each system reports

- **macOS: Low Power Mode.** `ProcessInfo.isLowPowerModeEnabled`, macOS 12.0+ (iOS 9.0+). The
  system then reduces CPU and GPU performance and screen brightness and pauses discretionary
  work. An app observes `NSProcessInfoPowerStateDidChange` on the default notification center
  and reads the property again. Apple: "If Low Power Mode is active, take appropriate steps to
  reduce activity in your app."
  (<https://developer.apple.com/documentation/foundation/processinfo/islowpowermodeenabled>)
- **Windows 11 24H2+: Energy saver**, which replaces battery saver. A person turns it on in Quick
  Settings, at a battery percentage they set, or always, plugged in too. While it is on, display
  brightness falls by 30 % and "Transparency effects on window backgrounds is disabled". On low
  battery it also blocks background apps, most telemetry and non-critical updates
  (<https://learn.microsoft.com/windows-hardware/design/component-guidelines/energy-saver>).
- **Windows: the reads.** `SYSTEM_POWER_STATUS::SystemStatusFlag` is 1 while battery saver is
  on, the page Microsoft's energy saver doc points to. `GUID_ENERGY_SAVER_STATUS` through
  `RegisterPowerSettingNotification` delivers an `ENERGY_SAVER_STATUS`: `ENERGY_SAVER_OFF`,
  `ENERGY_SAVER_STANDARD` ("Save energy if the user experience impact is minimal") or
  `ENERGY_SAVER_HIGH_SAVINGS` ("Save energy where possible"); the page marks it pre-release.
  `PowerRegisterForEffectivePowerModeNotifications` (Windows 10 1809+) calls back at once with
  the current effective power mode, which covers battery saver and the performance slider
  (<https://learn.microsoft.com/windows/win32/power/power-setting-guids>,
  <https://learn.microsoft.com/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications>).
- **Linux: the Power Profile Monitor portal.** `org.freedesktop.portal.PowerProfileMonitor`,
  version 1, has one read-only boolean, `power-saver-enabled`. The docs say it is "not a portal
  in the strict sense", and apps use it through GLib's `GPowerProfileMonitor`. GLib #2481: the
  portal-backed monitor subscribed to changes but never read the initial value, so an app
  started in power-saver mode did not notice. `ashpd` wraps it as `is_enabled()`
  (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.PowerProfileMonitor.html>,
  <https://gitlab.gnome.org/GNOME/glib/-/issues/2481>,
  <https://docs.rs/crate/ashpd/latest/source/src/desktop/power_profile_monitor.rs>).
- **Linux: underneath.** power-profiles-daemon's `org.freedesktop.UPower.PowerProfiles`
  `ActiveProfile` is `power-saver`, `balanced` or `performance`. UPower's `OnBattery` tells
  whether the machine runs on battery, which is not a request to save.
- Measured on a Linux desktop with `busctl`: the portal answers `power-saver-enabled` `b false`
  and `version` `u 1`; `ActiveProfile` `s "performance"`; `OnBattery` `b false`.
- **`mundy` 0.2.3** (the latest on crates.io, 2026-04-30) reads no power state: its features are
  accent color, color scheme, contrast, double-click time, reduced motion, reduced transparency
  and scrollbar visibility.
- In palantir's `Cargo.lock` already: `zbus` 5.19.0 (through `mundy`, Linux), `objc2-foundation`
  0.3.2, `windows` 0.62.2. `ashpd` is not.
- **`zbus` 6** (6.0.0 on 2026-10-16, 6.2.1 the latest on 2026-11-08) has no blocking API, and
  `mundy` 0.2.3 still depends on `zbus` 5, so a direct `zbus` 6 would build a second D-Bus stack.

- **What the reference implementations do.** GLib reads power-profiles-daemon's `ActiveProfile`
  (`"power-saver"`, compared exactly) on the system bus and watches the name, reporting off when
  it vanishes; it uses the portal only when `glib_should_use_portal()` holds, inside a sandbox
  (<https://gitlab.gnome.org/GNOME/glib/-/blob/main/gio/gpowerprofilemonitordbus.c>,
  <https://gitlab.gnome.org/GNOME/glib/-/blob/main/gio/gpowerprofilemonitorportal.c>). Its portal
  monitor reads the cached property once and follows `g-properties-changed`, ignoring a value of
  the wrong type. Windows: `RegisterPowerSettingNotification` asks an interactive app for a window
  handle; `PowerSettingRegisterNotification`'s callback gets the current value at once, but
  `PowerSettingUnregisterNotification` says nothing of a callback still running when it returns
  (<https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerpowersettingnotification>,
  <https://learn.microsoft.com/windows/win32/api/powersetting/nf-powersetting-powersettingregisternotification>,
  <https://learn.microsoft.com/windows/win32/api/powersetting/nf-powersetting-powersettingunregisternotification>).
  The Windows App SDK's `PowerManager` keeps its callbacks static with no context, and its
  `EnergySaverStatus` is `GUID_POWER_SAVING_STATUS`
  (<https://github.com/microsoft/WindowsAppSDK/blob/main/dev/PowerNotifications/PowerNotifications.cpp>).

### Measured with palantir's watches (2026-11-08)

- **Linux** (Arch, xdg-desktop-portal 1.22.1, power-profiles-daemon 0.30, a desktop): the first
  `power-saver-enabled` arrives 2.7 to 4.1 ms after the watch starts, `Reduced` when the profile
  was `power-saver` before it started. `powerprofilesctl set power-saver` and back arrive as
  `PropertiesChanged` signals at once. `zbus` 5.19's property stream on a caching proxy yields
  the current value first, as its docs say, but does not end on `Connection::close`; an iterator
  over the `PropertiesChanged` signal does, and the thread ends within 300 ms of the drop.
- **macOS 27.2** (an Apple laptop): `isLowPowerModeEnabled` reads in 0.3 ms; the first
  `addObserver` for `NSProcessInfoPowerStateDidChangeNotification` takes 218 ms, three runs in a
  row. Low Power Mode turned on and off by hand arrives as one notification each, on a thread
  with no name.
- **Windows 11 25H2** (build 26200, a desktop with no battery): `PowerSettingRegisterNotification`
  with `DEVICE_NOTIFY_CALLBACK` returns `ERROR_SUCCESS` for `GUID_ENERGY_SAVER_STATUS`
  (550E8400-E29B-41D4-A716-446655440000, which `windows` 0.62 does not declare) and for
  `GUID_POWER_SAVING_STATUS`, and each calls back within 1 ms with a 4-byte value, 0 with the
  saver off. `PowerRegisterForEffectivePowerModeNotifications` calls back with 3, high
  performance. `ESBATTTHRESHOLD` set to 100 on AC turns no saver on with no battery. Energy saver
  turned on in Quick Settings reads non-zero at registration, and each toggle by hand, off, on
  and off, arrives as one callback. `RegisterPowerSettingNotification` to a message-only window,
  the route palantir took after, delivers both settings' current values within 6 ms of the
  registration, and Energy saver turned on and off by hand arrives as `Reduced`, then `Full`, about
  4 s apart, one report each.

### What others do with it

- **WebKit** caps `requestAnimationFrame` at 30 fps in Low Power Mode, on purpose: WebKit bug
  215745 was closed as invalid, as the iOS 13 behavior without the cap was the bug, and a layout
  test expects about 33 fps
  (<https://bugs.webkit.org/show_bug.cgi?id=215745>,
  <https://webkit.googlesource.com/WebKit/+/2538ba1dc66c1a9ab694bc167c5d56293dae7e8e/LayoutTests/fast/animation/request-animation-frame-throttling-lowPowerMode.html>).
- **Chrome Energy Saver** (Chrome 110) turns on at 20 % battery or when unplugged, and limits
  background activity and visual effects such as smooth scrolling and video frame rates; no
  source found gives a number (<https://9to5google.com/2023/02/18/chrome-memory-energy-saver/>).
- **WinUI acrylic** falls back to its `FallbackColor` in Battery Saver
  (<https://learn.microsoft.com/windows/apps/design/style/acrylic>).
- **CSS** has no media feature for power saving, and the Battery Status API reports `charging`,
  `level` and the times to charge and discharge, not a mode
  (<https://drafts.csswg.org/mediaqueries-5/>,
  <https://developer.mozilla.org/docs/Web/API/Battery_Status_API>).
