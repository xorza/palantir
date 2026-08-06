<p align="center">
  <img src="https://raw.githubusercontent.com/xorza/palantir/master/assets/logo/palantir-mark.svg" width="256" alt="Palantir logo" />
</p>

<h1 align="center">Palantir</h1>

<p align="center">
  An immediate-mode GUI library for Rust — WPF-style two-pass layout, wgpu renderer.
</p>

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

The build sets `-C target-feature=+f16c` (see [Recommended build
flag](#recommended-build-flag)), worth ~6% of the CPU figures above.

---

A short screen recording of the
[showcase](https://github.com/xorza/palantir/tree/master/src/bin/showcase) tabs:

https://github.com/user-attachments/assets/0a403745-b841-4e17-bee9-fdbaad43c786

---

[Darkroom app](https://github.com/xorza/Darkroom)
![Darkroom app screenshot](https://raw.githubusercontent.com/xorza/palantir/master/docs/media/darkroom-screenshot.png)

## Highlights

- **Immediate-mode authoring**, builder-style widgets that read like prose.
- **WPF-contract two-pass layout** (measure → arrange) with flex-shrink
  sizing and a min-content floor.
- **wgpu rendering** with premultiplied-alpha linear-RGB throughout;
  sRGB encode happens on the swapchain.
- **Layered recording** — `Main` / `Popup` / `Modal` / `Tooltip` / `Debug`
  arenas painted bottom-up, hit-tested top-down.
- **Cross-frame work-skip cache** keyed on `(WidgetId, subtree_hash,
available_q)`; subtree hits blit last frame's measure result and skip
  recursion.
- **In-house text backend** on top of `cosmic-text` so the GPU upload
  path routes through palantir's staging belt.
- **`GpuView` — raw `wgpu` inside a widget.** Implement `GpuPaint` on your
  own renderer (a 3D scene, a custom shader) and hand it to
  `GpuView::new(paint)`; the framework owns an off-screen target sized to the
  widget's rect, runs your callback into it, and composites the result through
  the image pipeline — so it clips, rounds, and z-orders like any other widget.
  Mark a static view `.repaint(false)` and it goes undamaged (its paint is
  skipped) until something changes.

## Not yet implemented

Pre-1.0 — these are known gaps, not design rejections:

- **Accessibility** — no AccessKit / screen-reader support yet.
- **Italic + app-facing font loading** — text shapes in Regular or **Bold**
  (weight is wired through to shaping and rasterization), but there's no
  italic / oblique axis, and only the two bundled families (Inter, JetBrains
  Mono) exist — no arbitrary font registration yet.
- **Tab-key focus traversal** — focus exists (click-to-focus, programmatic
  `request_focus`), but `Tab` / `Shift+Tab` cycling does not.
- **Virtualized list / table** — `Scroll` records all children; no
  row-virtualized list or data table for large datasets.
- **Rich text** — one family / size / colour per `Text`; no inline spans.
- **SVG** — no SVG rendering (`Mesh` is the raw vector escape hatch).
- **RTL / bidirectional text** — right-to-left and mixed-direction scripts
  aren't supported yet.

## Zero per-frame allocation

Steady-state frames are heap-alloc-free after warmup. Per-frame data lives
on retained scratch (`RecordStore`, SoA columns on `Tree`, `CacheArena`)
that reuses capacity across frames; any new per-frame `Vec::new()` /
`HashMap` rebuild is treated as a regression and caught by the `alloc`
bench under `benches/`.

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

## Example

```rust,no_run
use palantir::{
    App, Button, Configure, Panel, Sizing, Text, Ui, WindowToken, WinitHost,
    WinitHostError,
};

struct Counter { clicks: u32 }

impl App for Counter {
    // `win` names which window is being drawn; switch on it for multi-window
    // apps. This one has a single window, so it's ignored.
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        Panel::vstack()
            .auto_id()
            .gap(8.0)
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Text::new(format!("clicks: {}", self.clicks)).auto_id().show(ui);
                if Button::new().label("click me").show(ui).left.clicked() {
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
[showcase](https://github.com/xorza/palantir/tree/master/src/bin/showcase)
for a tour of every widget:

```sh
cargo run --release --features showcase --bin showcase
```

To author your own widget from the public API, see
[`examples/custom_widget.rs`](https://github.com/xorza/palantir/blob/master/examples/custom_widget.rs) — a `Stepper`
built from `Element` + `Configure`, `Ui::widget_id` / `Ui::node` /
`Ui::add_shape` / `Ui::response_for`, with nothing reaching into crate
internals:

```sh
cargo run --example custom_widget
```

## License

Palantir is dual-licensed:

- **Open source / non-commercial use** —
  [GPL-3.0-or-later](https://github.com/xorza/palantir/blob/master/LICENSE).
  Free to use, modify, and redistribute, provided your combined work is also
  released under GPL-3.0-or-later with complete corresponding source.

- **Commercial use** — see
  [LICENSE-COMMERCIAL.md](https://github.com/xorza/palantir/blob/master/LICENSE-COMMERCIAL.md).
  If you want to ship Palantir as part of a proprietary, closed-source
  product, contact xxorza@gmail.com for a commercial license.

The bundled Inter and JetBrains Mono faces under `assets/fonts` are licensed
separately under the SIL Open Font License 1.1; their `*-OFL.txt` sit beside
them.

## Contributing

See
[CONTRIBUTING.md](https://github.com/xorza/palantir/blob/master/CONTRIBUTING.md).
All contributions are accepted under the
[Contributor License Agreement](https://github.com/xorza/palantir/blob/master/CLA.md),
which preserves the dual-license model by granting the maintainer the right to
relicense contributions (including commercially).
