# System preferences: color scheme and double-click time

Facts for the light and dark proposal (`.notes/showcase-proposal.md`), 2026-10-31.

## What palantir reads today

- No color scheme. Nothing reads winit's `Window::theme` or `WindowEvent::ThemeChanged`.
- The double-click time, in `src/host/winit/platform/`: Windows `GetDoubleClickTime` (the only
  use of the `windows` dependency), macOS `NSEvent.doubleClickInterval` (the only use of
  `objc2-app-kit`), Linux a fixed 400 ms.

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
- Measured on this machine (KDE Plasma, portal backends `kde` and `gtk`), with `busctl --user
  call … org.freedesktop.portal.Settings ReadOne`:

  | Namespace, key | Answer |
  |---|---|
  | `org.freedesktop.appearance color-scheme` | `u 1` |
  | `org.freedesktop.appearance reduced-motion` | `u 1` |
  | `org.freedesktop.appearance contrast` | `u 0` |
  | `org.freedesktop.appearance accent-color` | `(ddd) 0.584 0.733 0.090` |
  | `org.gnome.desktop.peripherals.mouse double-click` | `i 400` |
  | `org.kde.kdeglobals.KDE DoubleClickInterval` | not found |

  The GNOME double-click key reads through the portal, which `platform/linux.rs` says fails; on
  KDE it answers the GSettings value, not KDE's own setting.

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

## Contrast of the palettes (WCAG 2.2 ratios, computed 2026-10-31)

| Pair | `Palette::DEFAULT` (dark) | Light candidate |
|---|---|---|
| `text` on `window_bg` / `surround` / `element` / `element_mid` / `element_strong` | 15.45 / 13.83 / 9.84 / 8.04 / 6.55 | 9.88 / 8.95 / 10.14 / 8.17 / 7.22 |
| `text_muted` on `surround` / `element` / `element_mid` | 6.61 / 4.70 / **3.85** | 4.99 / 5.65 / 4.55 |
| accent on `window_bg` / `surround` / `element_mid` | 8.16 / 7.30 / 4.25 | 4.56 / 4.13 / 3.77 |
| `text` on the selection wash (accent at 64/255 over `element_mid`) | 5.48 | 6.04 |
| knob: `text` on accent | **1.89** | — |
| `window_bg` on accent / white on accent | 8.16 / 2.26 | 4.56 / 4.68 |
| rungs `window_bg`–`surround`, `surround`–`element_mid`, `element_mid`–`element_strong` | 1.118, 1.719, 1.227 | 1.104, **1.096**, 1.131 |

- Recipes that put `text_muted` on `element_mid`: a text field's placeholder
  (`theme/text_edit.rs`), a hovered menu row's shortcut (`theme/context_menu/menu_item.rs`).
- The smallest neutral gray that clears 4.5:1 on `#454545` is `#b2b2b2` (4.52:1).
- Ayu Light as it ships: muted `#828E9F` on `#F8F9FA` 3.15; accent `#F29718` on white 2.28;
  blue `#22A4E6` on `#EBEEF0` 2.40; ground `#FCFCFC` against `#F8F9FA` 1.03. Ayu's orange at its
  own hue and saturation clears 3:1 on `#e4e7ea` from `#ba710a`, and carries white at 4.5:1 from
  `#a66509` (4.68:1).

## `mundy` in the winit host (measured 2026-10-31, this machine)

- `Preferences::once_blocking(ColorScheme | DoubleClickInterval)` through the portal: 20.3 ms
  for the first read of a process (the D-Bus connection), then 1.0–1.4 ms; answers `Dark` and
  400 ms on this KDE desktop.
- A clean release build of `mundy` with palantir's features, alone in a scratch crate: 16.7 s on
  16 threads. palantir's `Cargo.lock` gains 46 package entries across the three targets.
- `Preferences::subscribe` runs its future on a thread `mundy` spawns ("mundy subscription
  thread", `src/async_rt.rs`) on every non-wasm target, so the callback is never on the main
  thread; `once_blocking` and `subscribe` are called on the main thread, as `mundy`'s caveats
  ask for macOS.
