# Palantir

An immediate-mode GUI library for Rust, with WPF-style two-pass layout and a
wgpu renderer.

Status: pre-1.0, under active development. APIs break freely.

![Showcase screenshot](docs/frame_bench.png)

Worst-case frame timing captured while resizing the window on a MacBook Air M5.

![Frame 146 profile](docs/frame-146-profile.png)

Steady-state cost per frame on `frame/cached_cpu` (ASUS ROG, i9-13980HX
P-core, 5.4 GHz): **~1.75 M instructions retired**, **~586 K cycles**,
**IPC ≈ 2.99** — measured via `perf stat -e cpu_core/instructions/`.

---

A short screen recording of the [showcase](src/showcase) tabs:

https://github.com/user-attachments/assets/73fd7143-087c-4895-a033-7644b184537f

---

[Darkroom app](https://github.com/xorza/Scenarium/tree/main/darkroom)
![Darkroom app screenshot](docs/Screenshot%202026-06-03%20at%2017.16.43.png)

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

## Zero per-frame allocation

Steady-state frames are heap-alloc-free after warmup. Per-frame data lives
on retained scratch (`FrameArena`, SoA columns on `Tree`, `CacheArena`)
that reuses capacity across frames; any new per-frame `Vec::new()` /
`HashMap` rebuild is treated as a regression and caught by the
`alloc_free` / `alloc_free_gpu` benches under `benches/`.

## Example

```rust
use palantir::{App, Button, Configure, Panel, Sizing, Text, Ui, WinitHost, WinitHostConfig};

struct Counter { clicks: u32 }

impl App for Counter {
    fn frame(&mut self, ui: &mut Ui) {
        Panel::vstack()
            .auto_id()
            .gap(8.0)
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Text::new(format!("clicks: {}", self.clicks)).auto_id().show(ui);
                if Button::new().label("click me").show(ui).clicked() {
                    self.clicks += 1;
                }
            });
    }
}

fn main() {
    WinitHost::new(WinitHostConfig::new("counter"), Counter { clicks: 0 }).run();
}
```

Run the bundled [showcase](src/showcase) for a tour of every widget:

```sh
cargo run --release
```

To author your own widget from the public API, see
[`examples/custom_widget.rs`](examples/custom_widget.rs) — a `Stepper`
built from `Element` + `Configure`, `Ui::widget_id` / `Ui::node` /
`Ui::add_shape` / `Ui::response_for`, with nothing reaching into crate
internals:

```sh
cargo run --example custom_widget
```

## License

Palantir is dual-licensed:

- **Open source / non-commercial use** — [GPL-3.0-or-later](LICENSE).
  Free to use, modify, and redistribute, provided your combined work is also
  released under GPL-3.0-or-later with complete corresponding source.

- **Commercial use** — see [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md).
  If you want to ship Palantir as part of a proprietary, closed-source
  product, contact xxorza@gmail.com for a commercial license.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All contributions are accepted
under the [Contributor License Agreement](CLA.md), which preserves the
dual-license model by granting the maintainer the right to relicense
contributions (including commercially).
