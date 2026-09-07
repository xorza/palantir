<p align="center">
  <img src="https://raw.githubusercontent.com/xorza/palantir/master/assets/logo/palantir-badge.svg" width="256" alt="Palantir logo" />
</p>

<h1 align="center">Palantir</h1>

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

Published on [crates.io](https://crates.io/crates/palantir); API reference on
[docs.rs](https://docs.rs/palantir).

Status: **beta** — feature-rich and usable, but still pre-1.0: the public
API can still change and break between releases.

![Frame bench timings](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/frame_bench.png)

Worst-case frame timing captured while resizing the window on a **MacBook Air M5**.

![tracy frame](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/tracy-frame.png)

The `frame` bench drives one synthetic app screen — every layout driver,
every widget, every `Shape` family — at 2560×1440, and runs each arm
twice: once as the deviceless CPU pipeline (record → measure → arrange →
cascade → damage → encode + compose), once as the full public path
through `OffscreenHost` with the GPU drained before the next iteration.

Intel Core i9-13980HX (Raptor Lake) with an RTX 4090 Laptop:

| arm         | CPU pipeline | CPU + GPU frame |
| ----------- | -----------: | --------------: |
| `cached`    |        95 µs |          170 µs |
| `partial`   |       103 µs |          252 µs |
| `scrolling` |       148 µs |          371 µs |
| `resizing`  |       246 µs |          578 µs |

Steady-state cost per frame on `frame/cached_cpu` (measured ~4.8 GHz,
~100 µs/frame): **~2.03 M instructions retired**, **~480 K cycles**,
**IPC ≈ 4.2**.

AMD Ryzen 7 6800U (Zen 3+) with its integrated Radeon 680M:

| arm         | CPU pipeline | CPU + GPU frame |
| ----------- | -----------: | --------------: |
| `cached`    |       130 µs |         1.12 ms |
| `partial`   |       145 µs |         1.37 ms |
| `scrolling` |       201 µs |         4.25 ms |
| `resizing`  |       317 µs |         5.38 ms |

Steady-state cost per frame on `frame/cached_cpu` (measured 4.59 GHz,
~130 µs/frame): **~2.03 M instructions retired**, **~596 K cycles**,
**IPC ≈ 3.41**.

Measured via `perf stat`, pinned to one core; the per-frame counts are a
differential between two measurement windows, so process startup cancels
out.

The build sets `-C target-feature=+f16c` (see [Recommended build flag](#recommended-build-flag)), worth ~6% of the CPU figures above.

---

A short screen recording of the
[showcase](https://github.com/xorza/palantir/tree/master/examples/showcase) tabs:

https://github.com/user-attachments/assets/0a403745-b841-4e17-bee9-fdbaad43c786

---

[Darkroom app](https://github.com/xorza/Darkroom)
![Darkroom app screenshot](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/darkroom-screenshot.png)

## Highlights

- **Immediate-mode authoring**, builder-style widgets that read like prose.
- **WPF-contract two-pass layout** (measure → arrange) with flex-shrink
  sizing and a min-content floor.
- **SVG artwork** — icons rasterize at their exact physical size into the
  icon atlas, so they stay crisp at any scale factor and cost nothing until
  first drawn. Gradients and filters included. A single-paint icon takes a
  tint whole; a colour one keeps its palette and takes the tint's alpha.
- **Headless test harness** — `UiHarness` runs the real UI with no window
  and no GPU. Click, drag, type, scroll and control the clock, then assert
  on what the frame did. See [Headless UI tests](#headless-ui-tests).
- **`GpuView` — raw `wgpu` inside a widget.** Implement `GpuPaint` on your
  own renderer and the framework runs it into a widget-sized off-screen
  target, then composites the result like any other image — so it clips,
  rounds and z-orders with everything else. `.repaint(false)` skips a static
  view's paint until something changes.

## Not yet implemented

Pre-1.0 — these are known gaps, not design rejections:

- **Accessibility** — no AccessKit / screen-reader support yet.
- **Tab-key focus traversal** — focus exists (click-to-focus, programmatic
  `Ui::set_focus`), but `Tab` / `Shift+Tab` cycling does not.
- **Rich text** — one family / size / colour per `Text`; no inline spans.
- **RTL / bidirectional text** — right-to-left and mixed-direction scripts
  aren't supported yet.

## Zero per-frame allocation

Steady-state frames are heap-alloc-free after warmup. Per-frame data lives on
retained scratch that reuses capacity across frames; any new per-frame
`Vec::new()` / `HashMap` rebuild is treated as a regression and caught by the
`alloc` test suite under `tests/`:

```sh
cargo test --test alloc
```

## Headless UI tests

Turn on the `internals` feature and `UiHarness` drives your interface with
no window, no GPU and no event loop. It records frames, feeds synthetic
input, and reads the result back. A whole interaction test costs
microseconds, so UI behaviour stays a plain `cargo test`.

- **Act like a user** — `click_on(id)`, `right_click_on`, `drag_to`,
  `scroll_lines`, `pinch`, `type_text("hi")`, `key`, `set_modifiers`.
- **Ask what happened** — arranged rect, centre, hit-test at a point,
  focus, hover, clipboard, and duplicate-id collisions.
- **Own the clock** — step animations frame by frame, or move past the
  double-click window. Every run is deterministic.
- **Pick the surface** — size, DPI scale, user scale, pixel snap, refresh
  rate. Test a 4K HiDPI layout on any machine.

```toml
[dev-dependencies]
palantir = { version = "*", features = ["internals"] }
```

```rust,ignore
use palantir::prelude::*;
use palantir::internals::UiHarness;

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

For pixels as well as behaviour, the `golden` feature adds golden-image
regression tests. `OffscreenHost` renders a frame into a `wgpu::Texture`,
and the comparison reports exactly which pixels moved.

## Recommended build flag

Colour, corner radii, spacing and shadow geometry are stored as f16. Without
F16C in the target features, each conversion takes a runtime feature check
into a `#[target_feature]` fn that can't inline into its caller — a spill and
a call every time.

```toml
# .cargo/config.toml
[target.'cfg(target_arch = "x86_64")']
rustflags = ["-C", "target-feature=+f16c"]
```

Worth **−5 to −8%** on the `frame` bench. Moves the CPU floor to Ivy Bridge
(2012), so it's the application's call — palantir keeps the runtime fallback
either way. `-C target-cpu=x86-64-v3` implies it, plus AVX2 and FMA.

## Install

```sh
cargo add palantir
```

The default features carry the winit host and the OS clipboard. With
`default-features = false` the only host left is `OffscreenHost`, which renders
into a `wgpu::Texture` you supply — that's the build for embedding palantir in
an app that already owns its window and event loop.

## Example

```rust,no_run
use palantir::prelude::*;
use palantir::{WinitHost, WinitHostError};

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

Run the bundled
[showcase](https://github.com/xorza/palantir/tree/master/examples/showcase)
for a tour of every widget:

```sh
cargo run --release --example showcase
```

Widget authoring lives in `palantir::widget`. The crate root is what an
application types; nothing in `widget` is needed to compose the widgets
Palantir ships. To write your own, see the showcase's **custom widget**
page —
[`examples/showcase/pages/custom_widget.rs`](https://github.com/xorza/palantir/blob/master/examples/showcase/pages/custom_widget.rs)
— a `Stepper` built entirely against the published API, reaching into no
crate internals.

## License

Licensed under either of

- [Apache License, Version 2.0](https://github.com/xorza/palantir/blob/master/LICENSE-APACHE)
- [MIT license](https://github.com/xorza/palantir/blob/master/LICENSE-MIT)

at your option.

The bundled Inter and JetBrains Mono faces under `assets/fonts` are licensed
separately under the SIL Open Font License 1.1; their `*-OFL.txt` sit beside
them.
