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
