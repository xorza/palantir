# Research for the crate review decisions (2026-10-26)

## Foreign re-exports

- Effective Rust, Item 24: a library whose API names a dependency's types
  re-exports that dependency whole, so a user gets the matching version; the
  cost is that the dependency's major bump is the library's major bump.
  <https://lurklurk.org/effective-rust/re-export.html>
- RFC 3516 (public/private dependencies): a type in a public signature makes
  the dependency public whether or not it is re-exported; Cargo gets
  `public = true`, and `exported_private_dependencies` warns on the rest.
  <https://rust-lang.github.io/rfcs/3516-public-private-dependencies.html>
- API guidelines discussion #176: re-export guidance was removed; open
  question. <https://github.com/rust-lang/api-guidelines/discussions/176>
- Practice: `egui-wgpu` has `pub use wgpu;` (0.22–0.35); `eframe` re-exports
  `egui`, `egui_wgpu`, and `wgpu` through it. <https://docs.rs/egui-wgpu>,
  <https://docs.rs/eframe>
- In the code: `lib.rs:143-147` gives the wgpu reason (`GpuPaint`, `Gpu`,
  `RenderTarget` name wgpu types); `lib.rs:351-355` re-exports only `IVec2`,
  `UVec2`, `Vec2` from glam; `prelude` (`lib.rs:186-194`) adds a second path
  for about 60 items; `#![warn(missing_docs)]` at `lib.rs:9` has a scope
  comment, no reason.

## Stroke `width` vs `thickness`

- Compose Material 3 `CircularProgressIndicator(strokeWidth)`,
  `ProgressIndicatorDefaults.CircularStrokeWidth`; Wear Compose the same;
  Flutter `CircularProgressIndicator.strokeWidth`.
  <https://kotlinlang.org/api/compose-multiplatform/material3/androidx.compose.material3/-circular-progress-indicator.html>,
  <https://api.flutter.dev/flutter/material/CircularProgressIndicator/CircularProgressIndicator.html>
- In the code: `SpinnerTheme::{thickness_ratio, min_thickness}` document
  themselves as a stroke width (`theme/spinner.rs:21-26`).

## Animation interruption

- CSS Transitions 2, "reversing shortening factor": a transition reversed
  part-way runs back over the time it ran, from a reversing-adjusted start.
  <https://drafts.csswg.org/css-transitions-2/>
- SwiftUI (WWDC23 "Animate with springs"): a retargeted spring keeps its
  velocity as the initial velocity toward the new target.
  <https://wwdcnotes.com/notes/wwdc23/10158>
- In the code: `animation_row.rs:81-104` zeroes a spring's velocity when it
  points away from the new target, "so a reversal doesn't overshoot", and a
  duration retarget restarts at `elapsed = 0`.

## `Platform` / `PLATFORM`

- In the code: published "so widgets branch on `PLATFORM`"
  (`common/platform.rs:3`); the only production reader is the winit input
  translator (`host/winit/window/mod.rs:140`).

## Layout contract

- In the code: `HugItem::share` (`layout/hug_item/mod.rs:34-54`) gives each
  item `lo + slack · (hi − lo) / Σ(hi − lo)`: the CSS 2.1 §17.5.2.2 auto
  table-layout interpolation. CSS Flexbox §9.7 shrinks by
  `flex-shrink × flex-base-size` and freezes min violations.

## Text key identity

- foldhash: `RandomState` seeds per process; "minimally DoS-resistant", not
  resistant to an attacker who can observe outputs or timing.
  <https://docs.rs/crate/foldhash/latest>
- std `RandomState` (SipHash-1-3) is keyed per process and needs no
  dependency; it is slower than FxHash or foldhash.
- egui's `GalleyCache` keys galleys by a `u64`; its docs say nothing of
  collisions. <https://doc.servo.org/epaint/text/fonts/struct.GalleyCache.html>
- In the code: `text/key.rs:33-34` (FxHash, fixed seed),
  `text/request.rs:268-275` (byte check in debug only).

## Double-click settings

- Windows: `GetDoubleClickTime` (ms, at most 5000, default 500).
  <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdoubleclicktime>
- Linux: no cross-desktop standard. XSETTINGS `Net/DoubleClickTime` (default
  250 ms) and `Net/DoubleClickDistance` (5 px), read by GTK; on Wayland GTK
  reads `org.gnome.desktop.peripherals.mouse double-click` through the
  Settings portal. <https://wiki.freedesktop.org/www/Specifications/XSettingsRegistry/>,
  <https://discourse.gnome.org/t/what-should-i-use-to-get-the-double-click-time/19917>
- egui: `InputOptions::{max_double_click_delay, max_click_dist}`, default
  delay 0.3 s, set by the app. <https://doc.servo.org/egui/struct.InputOptions.html>
- In the code: `input_state/capture.rs:19, 24` (500 ms, 5 px), and
  `internals/mod.rs:50` exports the window for harnesses.
