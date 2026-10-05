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

| arm | `b7b77cfd` | HEAD | target after this plan |
| --- | ---: | ---: | ---: |
| `cached_cpu` | 125.6 µs | 162.7 µs | 135–145 µs |
| `scrolling_cpu` | 194.2 µs | 225.7 µs | 195–205 µs |
| `scrolling_gpu` | 3.91 ms | 7.15–7.73 ms | 5.0–5.5 ms |
| `resizing_gpu` | 4.84 ms | 8.36–8.54 ms | 5.6–6.2 ms |

The targets are estimates from the profile shares below. Each step is measured
on its own before the next one starts.

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

## G3. A cache of blurred corners

### Problem

The integral costs about 2.2 ms per full frame on this fixture, and the
separate shadow pipeline did not reduce it. A cheaper analytic rule does not
exist at the same accuracy. The midpoint rule errs as
1/N², and Simpson's rule on the same weights is worse, because the across
factor is a steep step when σ is small:

| σ, r | midpoint N=12 | Simpson N=4 (13 evals) |
| --- | ---: | ---: |
| 2, 16 | 2.2·10⁻³ | 1.5·10⁻² |
| 0.5, 16 | 2.0·10⁻³ | 3.1·10⁻² |

### How others do it

WebRender and Skia render the blurred corner once into a cached texture and
draw the shadow as a nine-patch. Chrome's GPU box shadows use the Skia path.

### Design

Each corner term in `blurred_box_coverage` depends only on the pixel's
offset from the corner centre, on r and on σ. The four corners are
independent terms of an exact sum. So a table of one corner term per
`(r, σ)` replaces the loops with no approximation other than the
interpolation.

- Grid spacing h = σ / 8. Bilinear interpolation error, against the exact
  term:

  | σ | r = 4 | r = 16 |
  | --- | ---: | ---: |
  | 0.5 (h = 0.0625) | below 1.1·10⁻³ | below 1.4·10⁻³ |
  | 1 (h = 0.125) | 4.8·10⁻⁴ | 3.6·10⁻⁴ |
  | 2 (h = 0.25) | 7.0·10⁻⁴ | 4.5·10⁻⁴ |
  | 4 (h = 0.5) | 6.9·10⁻⁴ | 6.3·10⁻⁴ |

  That is more accurate than the current 12 slices (2.2·10⁻³ worst case),
  and under half an 8-bit step. The σ = 0.5 values are from h = 0.125 and
  will be measured again at h = 0.0625.
- The key is `(r, σ)` in physical pixels, exactly. A compute pass bakes a
  table with the exact integral at a high slice count.
- **Worst frame.** A key that is not baked yet draws with the analytic
  `fs_shadow` on that frame, and the bake has a fixed budget of texels per
  frame. So the first frame of a new shadow costs what it costs today, and no
  frame pays a large bake. An animated blur or radius never bakes, because
  each frame has a new key. Its key must be the same on two frames in a row
  before it bakes.
- Eviction runs on a frame stamp, with a fixed number of entries examined
  per frame, so no frame pays a full sweep.

### Validation

- A test compares table and analytic coverage over a grid of `(p, r, σ)`
  and asserts the bound above.
- The goldens change within that bound. Update them in the same commit and
  state the bound in the commit message.
- The table sampling belongs in `fs_shadow` alone, with its texture in a
  bind group that only the shadow pipeline uses.
- Expected gain: up to about 2 ms on `scrolling_gpu` and `resizing_gpu`.

## C3. An exact sRGB encode without a binary search

### Problem

`encode_byte` finds a byte with `partition_point` over 255 `f64` thresholds:
eight branches that the CPU cannot predict, for each channel. Gradient stops
(`Stop::new`) and mesh vertices encode on every frame.

### Design

A `u8` table of 1 634 entries, indexed by the exponent and the top seven
mantissa bits of the `f32`. Each bucket holds at most one threshold
(checked for all 255 thresholds), so

`byte = TABLE[i] + (y >= THRESHOLD_F32[TABLE[i]])`

is exact. `THRESHOLD_F32[k]` is the smallest `f32` at or above the `f64`
threshold, so the `f32` compare gives the same answer as the `f64` compare.
Values below the first bucket give 0, values at or above 1.0 give 255, and
NaN gives 0, as `partition_point` does today. Both tables are `const`.

### Validation

- An exhaustive test over every `f32` in `[0, 1]` (about 1.07·10⁹ values)
  against the current function. It runs only under `--ignored`, because it
  takes seconds. The normal test checks every threshold, the `f32` on each
  side of it, and the special values.
- Expected gain: 2–3% of `cached_cpu`.

## C4. Small per-widget costs added since August

Measure each one with the profile before you change it.

- **`AnimMap::animate`, 1.6%.** Find out if a settled look still does a map
  lookup for each widget in each frame. If so, a settled look reads its value
  without the animation map.
- **`Cascade::is_within`, 1.0%, and `Scopes::resolve`, 0.5%.**
  `is_within` does two hash lookups for each call, and `input/scope.rs`
  calls it in a filter over all rows. The rows already hold their node
  index, so compare node ranges directly.
- **`Widget::resolved` and `Forest::widget_id`.** The parent mix is one
  hash per widget per frame for every `id_salt` widget. Measure it before
  you change it.
- **Last-frame lookups by position** (`response_for` 3.2%, `is_within`,
  `AnimMap`) wait for a decision: see
  `.notes/PERF_REDESIGN_QUESTIONS.md`.

## C5. The worst-case compose bound, at a lower constant

### Problem

`86739680` bounds the worst-case compose with a tiled occlusion index and
`RectGrid`. The default fixture pays about 100 K instructions per frame for
it. The bound is correct and stays.

### Design

Measure the index's own cost per group size first. If small groups pay for
the index, they use the plain scan up to the size where the index wins. That
is the same rule that the higher-kind tiers already use at 32 rects. The
worst case keeps its bound, because large groups still use the index.

### Validation

- The composer's pruning tests, plus a test that the threshold does not
  change which quads survive.
- Expected gain: up to 1.5% of `cached_cpu`.

---

# Part 2: Implementation order

Each step is one commit with its tests. Measure each step with the A/B
protocol in `benches/AGENTS.md` (ABBA, pinned core, `setarch -R`, governor
`performance`), and record the result in the commit message.

1. **C3, exact sRGB table.** Local change with an exhaustive test.
2. **C4 items**, each one after a fresh profile.
3. **C5, compose threshold**, after a measurement of the index cost.
4. **G3, corner cache.** The largest GPU gain left.
5. **Docs.** Update the `README.md` tables and the `perf stat` paragraph
   with full runs. Add the `git archive` mtime trap from
   `.notes/FRAME_BENCH_REGRESSION.md` to `benches/AGENTS.md`.

## Not in this plan

- The record path's locality (`open_node`, `Node` packing, `ShapeRecord`
  size, the rollup row). The earlier audit measured these and dropped them.
- The OS difference on GPU `cached` and `partial` (about +20%). It is
  outside the crate.
- Integer formatting in the fixture (about 4%). It is the fixture's own
  work.
