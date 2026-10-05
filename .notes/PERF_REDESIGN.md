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

## G3. A cache of corner cutouts

### Done: the cutout form

From σ = 0.25 up, the shadow coverage is now the sharp box (closed form)
less four corner cutouts, each an angle integral over 12 midpoint nodes.
Against the exact integral it errs at most 1.1·10⁻³ there, where the
outline form erred 2.3·10⁻³, and it costs about half the kernel
evaluations: ABBA `scrolling_gpu` 7.33 → 6.05 ms, `resizing_gpu` 8.19 →
7.08 ms. Below σ = 0.25 the outline form stays, because the cutout's
density loses to the pixel box's step there.

### Problem

The cutout integral still costs about 1.1 ms per full frame on this fixture
(`scrolling_gpu` 6.05 ms against 4.91 ms before `e0500557`).

### How others do it

WebRender and Skia render the blurred corner once into a cached texture and
draw the shadow as a nine-patch. Chrome's GPU box shadows use the Skia path.

### Design

The cutout `U(q; r, σ)` is one function for all four corners (they are its
reflections), and it is zero more than `reach` away from its `r`×`r`
square, so one table per `(r, σ)` over `[−reach, r + reach]²` replaces the
12 nodes with four texel loads.

- Grid spacing h = σ / 8. Bilinear interpolation of the outline form's
  corner term measured at most 7·10⁻⁴ at that spacing; measure it again for
  the cutout, and choose h so that table and interpolation together stay
  under the current 1.1·10⁻³.
- `R32Float` texels with a manual bilinear filter: the format is not
  filterable on every backend, and 16-bit texels would add 2.4·10⁻⁴.
- The key is `(r, σ)` as the shader computes them — the spread-adjusted,
  fitted radius in the shader's units — so the CPU side must compute them
  the same way, and a shadow instance must carry where its four tables are.
- **Worst frame.** A key that is not baked yet draws with the analytic
  cutout on that frame, and the bake has a fixed budget of texels per
  frame. An animated blur or radius never bakes, because each frame has a
  new key: a key must hold for two frames in a row before it bakes.
- Eviction runs on a frame stamp, with a fixed number of entries examined
  per frame, and the allocator never repacks, so no frame pays a sweep.

### Validation

- A test compares table and analytic coverage over a grid of `(p, r, σ)`
  and asserts the bound.
- Expected gain: up to about 1 ms on `scrolling_gpu` and `resizing_gpu`.

## C4. Small per-widget costs added since August

Measure each one with the profile before you change it.

- **`AnimMap::animate`, 1.6%.** Find out if a settled look still does a map
  lookup for each widget in each frame. If so, a settled look reads its value
  without the animation map.
- **`Cascade::is_within`, 1.0%: measured and dropped for the scope scans.**
  Scope rows that carried their node, and scans that looked up only the
  queried widget, made `cached_cpu` 0.9% slower (ABBA, +0.92% / −0.87%):
  `Scopes::reader`'s memo already spares the per-chord scans. The rest of
  the cost is in callers that ask about one widget, `is_focus_within` and
  `is_hover_within`, which belong to Q1's positional reads.
- **Last-frame lookups by position** (`response_for` 3.2%, `is_within`,
  `AnimMap`) wait for a decision: see
  `.notes/PERF_REDESIGN_QUESTIONS.md`.

---

# Part 2: Implementation order

Each step is one commit with its tests. Measure each step with the A/B
protocol in `benches/AGENTS.md` (ABBA, pinned core, `setarch -R`, governor
`performance`), and record the result in the commit message.

1. **C4 items**, each one after a fresh profile.
2. **G3, cutout cache.** The largest GPU gain left. Waits for a decision:
   see Q2 in `.notes/PERF_REDESIGN_QUESTIONS.md`.
3. **Docs.** Update the `README.md` tables and the `perf stat` paragraph
   with full runs. Add the `git archive` mtime trap from
   `.notes/FRAME_BENCH_REGRESSION.md` to `benches/AGENTS.md`.

## Not in this plan

- The record path's locality (`open_node`, `Node` packing, `ShapeRecord`
  size, the rollup row). The earlier audit measured these and dropped them.
- The OS difference on GPU `cached` and `partial` (about +20%). It is
  outside the crate.
- Integer formatting in the fixture (about 4%). It is the fixture's own
  work.
