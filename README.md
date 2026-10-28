<p align="center">
  <img src="https://raw.githubusercontent.com/xorza/palantir/master/assets/logo/palantir-badge.svg" width="256" alt="Palantir logo" />
</p>

<p align="center">
  An immediate-mode GUI library for Rust — WPF-style two-pass layout, wgpu renderer.
</p>

<p align="center">
  <a href="https://crates.io/crates/palantir"><img src="https://img.shields.io/crates/v/palantir.svg" alt="crates.io" /></a>
  <a href="https://docs.rs/palantir"><img src="https://img.shields.io/docsrs/palantir" alt="docs.rs" /></a>
  <a href="https://github.com/xorza/palantir/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/xorza/palantir/ci.yml?branch=master&amp;label=CI" alt="CI" /></a>
  <a href="https://crates.io/crates/palantir"><img src="https://img.shields.io/crates/d/palantir.svg" alt="downloads" /></a>
  <a href="https://github.com/xorza/palantir#license"><img src="https://img.shields.io/crates/l/palantir.svg" alt="license" /></a>
</p>

I wanted a cross-platform GUI library — simple yet powerful, WPF-style layout
declaration, good-looking and fast. I made it.

> **Beta.** Feature-rich and usable, but pre-1.0: the public API can still
> change between releases.

## Highlights

- **Immediate-mode authoring** — builder-style widgets that read like prose.
  `record` describes the whole UI every frame; there is no retained widget
  tree to keep in sync.
- **WPF-contract two-pass layout** — measure, then arrange: Hug items shrink
  between their min- and max-content widths as auto table columns do, Fill
  items share the rest as WPF star tracks do, and nothing shrinks below its
  min-content floor.
- **Fast** — a full 2560×1440 app screen costs 66–107 µs of CPU per frame
  when nothing changed and 175–290 µs while the window resizes, and a
  steady-state frame allocates nothing. See [Performance](#performance).
- **Widgets included** — buttons, text and text editing, sliders, checkboxes,
  combo boxes, scroll and pan-zoom views, grids, tabs, a dock, popups, modals,
  tooltips and context menus, all styled from one serializable `Theme`.
- **SVG artwork** — icons rasterize at their exact physical size into the
  icon atlas, so they stay crisp at any scale factor and cost nothing until
  first drawn. Gradients and filters included.
- **Keyboard all the way** — `Tab` / `Shift+Tab` walk every interactive
  widget, dialogs and menus trap focus and give it back on close, the arrows
  walk menus and any `arrow_focus` group, and widgets take the keys WAI-ARIA
  gives their role. The focus ring shows only for keyboard focus.
- **IME text** — an input method composes inline in a `TextEdit`,
  underlined at the caret, and its candidate list follows the caret.
- **Raw `wgpu` inside a widget** — implement `GpuPaint` and a `GpuView`
  runs your renderer into a widget-sized target, then composites it like any
  image: it clips, rounds and z-orders with everything else.
- **Embeddable** — the winit host owns the window by default; `OffscreenHost`
  renders into a `wgpu::Texture` you supply, inside an event loop you own.
- **Headless UI tests** — `UiHarness` runs the real UI with no window and no
  GPU. See [Testing](#testing).

## See it

A short screen recording of the
[showcase](https://github.com/xorza/palantir/tree/master/examples/showcase) tabs:

https://github.com/user-attachments/assets/66d64697-de78-4e01-be3e-8874b5a00b0b

Built with Palantir: [Darkroom](https://github.com/xorza/Darkroom), a
node-graph editor for image and data pipelines.

<img width="2285" height="1976" alt="Screenshot_20261029_000442" src="https://github.com/user-attachments/assets/8ae87047-429d-4aef-8cbc-0d2dfe8a8e8c" />


## Getting started

```sh
cargo add palantir
```

The default features carry the winit host and the OS clipboard. With
`default-features = false` the only host left is `OffscreenHost`, the build
for an app that already owns its window and event loop.

```rust no_run
use palantir::{
    App, Button, Configure, Panel, Sizing, Text, Ui, WindowToken, WinitHost, WinitHostError, fmt,
};

struct Counter { clicks: u32 }

impl App for Counter {
    // `win` names which window is being drawn; switch on it for multi-window
    // apps. This one has a single window, so it's ignored.
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        Panel::vstack()
            .gap(8.0)
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                // `fmt!` formats into the frame's text arena — no `String`.
                Text::new(fmt!(ui, "clicks: {}", self.clicks)).show(ui);
                if Button::new().label("click me").show(ui).clicked() {
                    self.clicks += 1;
                }
            });
    }
}

fn main() -> Result<(), WinitHostError> {
    WinitHost::builder(WindowToken(0))
        .title("counter")
        .build(|_ui, _host| Counter { clicks: 0 })?
        .run()
}
```

For a tour of every widget, run the bundled
[showcase](https://github.com/xorza/palantir/tree/master/examples/showcase):

```sh
cargo run --profile release-dev --example showcase
```

**Your own widgets.** The crate root is what an application types;
`palantir::widget` is what a widget is built from. The showcase's
**custom widget** page,
[`examples/showcase/pages/custom_widget.rs`](https://github.com/xorza/palantir/blob/master/examples/showcase/pages/custom_widget.rs),
builds a `Stepper` entirely against the published API.

## Performance

The `frame` bench drives one synthetic app screen — every layout driver,
every widget, every `Shape` family — on a 2560×1440 surface at 2× scale.
Each scenario runs two ways: the deviceless **CPU pipeline** (record →
measure → arrange → cascade → damage → encode + compose), and the full
public path through `OffscreenHost`, the **CPU + GPU frame**, with the GPU
drained before the next iteration.

|                                    | Intel i9-13980HX<br>RTX 4090 Laptop | AMD Ryzen 7 6800U<br>Radeon 680M | Apple M5, MacBook Air<br>10-core GPU |
| ---------------------------------- | ----------------------------------: | -------------------------------: | -----------------------------------: |
| **CPU pipeline**                   |                                     |                                  |                                      |
| `cached`                           |                               79 µs |                           107 µs |                                66 µs |
| `partial`                          |                              113 µs |                           153 µs |                                93 µs |
| `scrolling`                        |                              149 µs |                           196 µs |                               116 µs |
| `resizing`                         |                              227 µs |                           290 µs |                               175 µs |
| **CPU + GPU frame**                |                                     |                                  |                                      |
| `cached`                           |                               80 µs |                           111 µs |                                66 µs |
| `partial`                          |                              296 µs |                          1.53 ms |                               361 µs |
| `scrolling`                        |                              414 µs |                          2.50 ms |                               866 µs |
| `resizing`                         |                              572 µs |                          2.85 ms |                              1.00 ms |
| **Per `cached` CPU frame**         |                                     |                                  |                                      |
| instructions retired               |                              1.72 M |                           1.71 M |                               1.74 M |
| cycles                             |                               403 K |                            490 K |                                285 K |
| IPC                                |                                4.26 |                             3.49 |                                  6.1 |
| clock                              |                           ~5.07 GHz |                         4.58 GHz |                             4.32 GHz |

- **Scenarios.** `cached` changes nothing, so layout hits its cache and
  damage skips the paint. `partial` changes one counter, a small damage rect.
  `scrolling` moves a transform. `resizing` changes the surface size every
  frame, so every measure runs again.
- **Counters.** The x86 counts come from `perf stat`, pinned to one core; the
  M5 counts from `/usr/bin/time -l`, unpinned, as macOS has no core affinity.
  Each count is the difference between two measurement windows, so process
  startup cancels out.
- **Build.** The x86 build sets `-C target-cpu=x86-64-v3` (see
  [Recommended build flag](#recommended-build-flag)); an aarch64 build sets
  no flag.

The bench screen, as the showcase's frame bench page draws it:

![Frame bench screen](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/frame_bench.png)

The worst-case frame in a Tracy capture of a window resize on the MacBook Air M5:

![Tracy capture of one frame](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/tracy-frame.png)

### Zero per-frame allocation

A steady-state frame allocates nothing after warmup. Per-frame data lives on
retained scratch that keeps its capacity across frames, and the `alloc`
test suite fails on any new per-frame allocation.

### Recommended build flag

Color, corner radii, spacing and shadow geometry are stored as f16. Without
F16C in the target features, each conversion takes a runtime feature check
into a `#[target_feature]` fn that can't inline into its caller — a spill and
a call every time. x86-64-v3 carries F16C, plus AVX2 and FMA:

```toml
# .cargo/config.toml
[target.'cfg(target_arch = "x86_64")']
rustflags = ["-C", "target-cpu=x86-64-v3"]
```

F16C alone is worth **5–8%** on the `frame` bench. The flag moves the CPU
floor to Haswell (2013), so it's the application's call — palantir keeps the
runtime fallback either way. An application that must run on older CPUs can
set `-C target-feature=+f16c` instead, for an Ivy Bridge (2012) floor.

## Testing

### Headless UI tests

Turn on the `internals` feature and `UiHarness` drives your interface with
no window, no GPU and no event loop. It records frames, feeds synthetic
input, and reads the result back. A whole interaction test costs
microseconds, so UI behavior stays a plain `cargo test`.

- **Act like a user** — `click_on(id)`, `right_click_at(pos)`, `drag_to`,
  `scroll_lines`, `pinch`, `type_text("hi")`, `key`, `set_modifiers`.
- **Ask what happened** — a widget's arranged rect, center and response,
  the widget at a point, focus, and the clipboard.
- **Own the clock** — step animations frame by frame, or move past the
  double-click window. Every run is deterministic.
- **Pick the surface** — size, DPI scale, user scale, pixel snap, refresh
  rate. Test a 4K HiDPI layout on any machine.

```toml
[dev-dependencies]
palantir = { version = "*", features = ["internals"] }
```

```rust
use palantir::glam::UVec2;
use palantir::internals::ui_harness::UiHarness;
use palantir::{Button, Configure, Ui, WidgetId};

let inc = WidgetId::from_hash("inc");
let mut clicks = 0_u32;
let mut screen = |ui: &mut Ui| {
    if Button::new().id(inc).label("click me").show(ui).clicked() {
        clicks += 1;
    }
};

let mut h = UiHarness::new(UVec2::new(400, 200));
h.prime(2, &mut screen);
h.click_on(inc);
h.frame(&mut screen);

assert_eq!(clicks, 1);
```

`internals` is not a supported API: it can change in any release.

### Golden images

For pixels as well as behavior, the `golden` feature adds golden-image
regression tests. `OffscreenHost` renders a frame into a `wgpu::Texture`,
and the comparison reports exactly which pixels moved.

## Not yet implemented

Known gaps, not design rejections:

- **Accessibility** — no AccessKit / screen-reader support yet.
- **Rich text** — one family / size / color per `Text`; no inline spans.
- **Visual caret motion in bidirectional text** — `TextEdit`'s arrow keys
  step graphemes in logical order, so inside a right-to-left run `←` moves
  the caret visually rightward.

## License

Licensed under either of

- [Apache License, Version 2.0](https://github.com/xorza/palantir/blob/master/LICENSE-APACHE)
- [MIT license](https://github.com/xorza/palantir/blob/master/LICENSE-MIT)

at your option.

The bundled Inter and JetBrains Mono faces under `assets/fonts` are licensed
separately under the SIL Open Font License 1.1; their `*-OFL.txt` sit beside
them.
