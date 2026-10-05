# Frame performance: redesign and implementation plan

This plan recovers the frame cost that `.notes/FRAME_BENCH_REGRESSION.md`
traces to commits after `b7b77cfd`. It changes no public API. It keeps every
behaviour that those commits added on purpose: the exact sRGB encode, the
exact Gaussian shadow, the unique-id guarantee for widgets that resolve before
they record, and the bounded worst-case compose.

When a step is done, delete it. When a design has no steps, delete the design.

Measured on the Ryzen 7 6800U / Radeon 680M, `x86-64-v3`, core 2 pinned,
`setarch -R`. HEAD is `77719e8a`.

---

## Result

| arm | `b7b77cfd` | `77719e8a` | target | now |
| --- | ---: | ---: | ---: | ---: |
| `cached_cpu` | 125.6 µs | 162.7 µs | 135–145 µs | 140.6 µs |
| `scrolling_cpu` | 194.2 µs | 225.7 µs | 195–205 µs | 203.7 µs |
| `scrolling_gpu` | 3.91 ms | 7.15–7.73 ms | 5.0–5.5 ms | 5.40 ms |
| `resizing_gpu` | 4.84 ms | 8.36–8.54 ms | 5.6–6.2 ms | 6.56 ms |

"Now" is the median of three full runs at `c4a28240`, with every step in.

The targets were estimates from the profile shares below. Each step was
measured on its own before the next one started.

The plan does not reach the `b7b77cfd` numbers. The rest of the difference is
the cost of features that the later commits added on purpose: exact colour,
exact shadows, the worst-case compose bound, and correct ids. This plan keeps
those features and removes only their excess cost.

---

## Findings

### GPU: the shadow integral

`e0500557` replaced one `erf` per shadow fragment with an exact integral over
each rounded corner: 4 corners × 2 arc halves × 12 slices, with two
`filter_cdf` per slice. The shadow branch is part of the one quad fragment
shader that draws every rectangle. RADV statistics (`RADV_DEBUG=shaderstats`)
for that shader:

| build | VGPRs | waves/SIMD | instructions | code |
| --- | ---: | ---: | ---: | ---: |
| `50025ce6` (before) | 40 | 24 | 836 | 4.2 KB |
| HEAD | 56 | 18 | 7 851 | 50 KB |

A GPU allocates registers for the worst path of a shader, so every quad
pipeline carried those 56 VGPRs and 50 KB. But that is not where the time
goes. With the shadows moved to their own fragment entry (`fs_shadow`, done),
the quad shader is back at 40 VGPRs and 3.8 KB, and the GPU arms did not
change (ABBA: `scrolling_gpu` 7.35 / 7.42 / 7.42 / 7.33 ms). The cost is the
shadow arithmetic itself. A slice-count experiment shows its size:

| build | `scrolling_gpu` | `resizing_gpu` | shader |
| --- | ---: | ---: | --- |
| `50025ce6` | 4.91 ms | 5.62 ms | 40 VGPRs, 4.2 KB |
| HEAD, 12 slices | 7.15 ms | 8.36 ms | 56 VGPRs, 50 KB |
| HEAD, 4 slices | 6.35 ms | 7.14 ms | 56 VGPRs, 85 KB |
| HEAD, 1 slice | 6.10 ms | 6.89 ms | 56 VGPRs, 49 KB |

Going from 12 slices to 1 saves about 1.05 ms. At one slice, a fragment near
a corner still evaluates about 20 `filter_cdf`, against one `erf` before
`e0500557`, so the 1.2 ms that remains is arithmetic too.

The full-frame fragment count increased by only 4% (8.07 M → 8.39 M), from
the wider shadow reach of `2adee187`.

### CPU: instructions per frame

`perf stat`, a differential of a 2 s and a 6 s window, at 4.64 GHz:

| commit | instructions/frame | cycles/frame | IPC |
| --- | ---: | ---: | ---: |
| `b7b77cfd` | 1.98 M | 583 K | 3.40 |
| `91e17114` | 2.06 M | 630 K | 3.28 |
| `ced36fc3` sRGB and f16 colours | 2.11 M | 663 K | 3.19 |
| `d68485de` id reservation | 2.27 M | 689 K | 3.29 |
| `86739680` worst-case compose bound | 2.42 M | 739 K | 3.27 |
| HEAD | 2.45 M | 755 K | 3.24 |

The frame is still not memory-bound (IPC 3.2–3.4), as the earlier locality
audit found (`0bb9f64d:.notes/OPTIMIZATION_PLAN.md`). The cost is the
instruction count during recording.

Self time at HEAD, `cached_cpu` (only the items this plan acts on):

| symbol | share | in `b7b77cfd` |
| --- | ---: | ---: |
| `SeenIds::resolve` | 6.5% | inlined in `Ui::widget` |
| `WidgetId::auto` | 5.4% | 5.1% (`auto_stable`) |
| `RgbaF32::to_srgba_u8` | 3.1% | — |
| `AnimMap::animate` | 1.6% | — |
| `Widget::resolved` | 1.5% | — |
| `Cascade::is_within` + `Scopes::resolve` | 1.5% | — |
| compose (`ComposeSession`, `RectGrid`, occlusion) | 6.9% | 5.1% |

Recording, the id path and the rollups together are about two thirds of the
CPU frame. The fixture has about 1 060 nodes, so the frame spends about 710
cycles per node.

---

# Part 1: Designs

---

# Part 2: Implementation order

Every step is done. Each was one commit with its tests, measured with the
A/B protocol in `benches/AGENTS.md`, its result in the commit message.

## Not in this plan

- The record path's locality (`open_node`, `Node` packing, `ShapeRecord`
  size, the rollup row). The earlier audit measured these and dropped them.
- The OS difference on GPU `cached` and `partial` (about +20%). It is
  outside the crate.
- Integer formatting in the fixture (about 4%). It is the fixture's own
  work.
