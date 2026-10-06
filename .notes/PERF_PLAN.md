# Frame performance: design and plan

Measured on 2026-10-06 on the Ryzen 7 6800U / Radeon 680M
(`beelink-ser5-dev`), on the working tree over `77719e8a` (cutout tables
and in-step ids included). Background: `.notes/FRAME_BENCH_REGRESSION.md`.
All experiments ran on a copy of the tree with runtime switches. No
experiment code is in the tree.

## Result

- **GPU, shading:** the shadow shader is the whole regression of
  `scrolling_gpu` and `resizing_gpu`. The shadow grid (in) removed 0.75–0.9 ms of it.
- **GPU, copy-out:** the GPU arms now present the way the desktop does
  (`DirectAdaptive`, see `benches/AGENTS.md`). On that strategy a skip
  frame copies nothing, so `cached_gpu` is the CPU frame alone, and
  `partial_gpu` is the damage paint plus one full copy-out (M1).
- **CPU:** no single cause. The largest item, folding the rollups at
  `close_node` (C1), made the frame slower and was reverted. Three small
  internal changes remain, estimated at 2–3.5 µs, so `cached_cpu` stays
  above the old README unless the C5 investigations find more.

| arm | old README | now (desktop strategy) | estimate |
| --- | ---: | ---: | ---: |
| `cached_cpu` | 130 µs | 140.3 µs | 137–138 µs |
| `partial_cpu` | 145 µs | 155.2 µs | 152–153 µs |
| `scrolling_cpu` | 201 µs | 203.7 µs | 200–202 µs |
| `resizing_cpu` | 317 µs | 304.6 µs | 301–303 µs |
| `cached_gpu` | 1.12 ms | 0.131 ms | 0.125–0.128 ms |
| `partial_gpu` | 1.37 ms | 1.64 ms | ≈ 1.6 ms |
| `scrolling_gpu` | 4.25 ms | 4.86 ms | ≈ 4.0 ms (grid in) |
| `resizing_gpu` | 5.38 ms | 6.00 ms | ≈ 5.2 ms (grid in) |

The old README GPU numbers were measured with `BackbufferCopy`, so they
compare with the CPU columns only.

The GPU arms vary by up to ±10% between runs, so a GPU estimate is only
as good as its 30-sample ABBA run.

## GPU: shading

| experiment (30 samples) | `scrolling_gpu` | `resizing_gpu` |
| --- | ---: | ---: |
| as is | 5.48 ms | 6.41 ms |
| shadows not drawn | 2.86 ms | 3.70 ms |
| shadows drawn, `fs_shadow` returns 0 at once | 3.54 ms | 4.73 ms |
| shadows drawn, no corner cutouts | 5.03 ms | 6.38 ms |

- The shadows cost 2.6–2.7 ms. Without them, these two arms are faster
  than the old README.
- 0.7–1.0 ms is rasterization and blending of shadow fragments.
- 1.7–1.9 ms is shader math. The corner cutouts are only 0.03–0.45 ms of
  it. The rest is the sharp-box term: four `filter_cdf` (each two `erf`
  and two `exp`) at every fragment.

The fixture draws 10 drop shadows, σ = 16–20 px, radius 12–20 px, offset
(0, 4–14), spread 0. They cover 4.74 M visible pixels and add 4.1 M
fragment invocations (8.39 M against 4.26 M) on a 3.7 M pixel surface.
**2.72 M of those pixels (57%) are inside the source card**, inset by its
largest radius plus `AA_HALF_WIDTH`. There, `fs_shadow` returns
`vec4(0)`: each such fragment rasterizes, evaluates the source SDF, and
blends zero, which still reads and writes 8 bytes of the target. At
1440p that is about 22 MB of memory traffic per full frame for no pixel.

## GPU design

### G3. Only if the edges dominate what is left: a baked edge profile

The shadow grid is in: a shadow skips the pixels its source hides, and
takes the two-term edge form or the full cover where the cutoff at
`reach` makes them exact. On the fixture it cut fragments from 8.39 M to
5.85 M, `scrolling_gpu` from 4.9 to 4.0 ms and `resizing_gpu` from 5.95
to 5.20 ms (ABBA, both directions). Shadows are still about 1.1 ms of a
full frame, against about 0.5 ms for every other quad.

`filter_cdf(u, σ)` depends only on `u` and σ. One atlas row per distinct
σ, baked by the cutout pass, gives the edge profile with two texel loads
and a lerp. Do G3 only if a profile shows the edge terms as a large part
of the remaining shadow cost.

## GPU: the copy-out

The winit host presents with `PresentStrategy::DirectAdaptive`
(`src/host/winit/runtime.rs`). That strategy never reads a swapchain
image's earlier contents:

- a skip frame does not present;
- a full frame renders directly into the swapchain image;
- a partial frame paints the damage into palantir's own backbuffer and
  then copies the whole backbuffer onto the swapchain image.

The frame bench's GPU arms use `OffscreenHost` with the default
`retained_target(false)`, which is `PresentStrategy::BackbufferCopy`: the
path for a fresh texture at each call (screenshots, the visual harness).
It renders every frame into the backbuffer and copies it out, and a skip
frame also copies it out. `retained_target(true)` selects
`DirectAdaptive`.

| arm (30 samples) | `BackbufferCopy` | `DirectAdaptive` |
| --- | ---: | ---: |
| `cached_gpu` | 1.41 ms | 0.13 ms |
| `partial_gpu` | 1.59 ms | 1.54 ms |
| `scrolling_gpu` | 5.63 ms | 4.83 ms |
| `resizing_gpu` | 6.53 ms | 6.00 ms |

- A skip frame under `BackbufferCopy` costs 1.27 ms for the copy and the
  GPU wake-up. The desktop never does this.
- A full frame under `BackbufferCopy` costs about 0.5–0.8 ms more than a
  direct render.
- A partial frame costs the same under both, because both copy the whole
  backbuffer out. A blit (`Backbuffer::draw_onto`) in place of the copy
  is not faster on RADV: 1.67 ms against 1.59 ms.

### M1. Less memory traffic without trusting swapchain contents

With no trust in a swapchain image's earlier contents, a presented frame
must write every pixel of that image. That write is the floor. Today:

| frame | today | floor |
| --- | --- | --- |
| skip | no present | no present |
| full | render direct | render direct |
| partial | full read + full write (copy) | full write |

The partial copy is two times the floor, and the only way below it would
be to render the undamaged pixels again, which costs more than reading
them. So the gains are elsewhere:

- **The shadow hole skip** is in: it removed 2.5 M blended fragments,
  about 20 MB of traffic per full frame.
- **The resync after a direct frame.** A full frame renders directly and
  leaves the backbuffer stale. The next partial frame then repaints the
  whole frame into the backbuffer before it copies out. A frame sequence
  of full, partial, full, partial (a hover over an animated region, for
  example) pays a full repaint plus a full copy at every partial frame.
  Choosing per full frame between "direct" and "via the backbuffer, then
  copy" by the last frames' history would avoid that. This needs a bench
  arm that alternates full and partial frames before any design, because
  no arm measures that sequence today.
- **A timestamp around the copy-out.** The copy runs outside the render
  pass, so `GpuPassStats` does not show it. One timestamp pair around it
  separates the copy from the GPU wake-up, and tells whether a smaller
  copy would help at all.

## CPU: where the time goes

Profiles with frame pointers, `cached_cpu`, 5 s each, of `b7b77cfd` (the
old README commit) and of the working tree, both with `x86-64-v3` (126.3
and 146.9 µs per frame with frame pointers). In µs per frame:

| function | old | now | change |
| --- | ---: | ---: | ---: |
| `compute_rollups` (inlined into `post_record`) | 12.6 | 16.6 | +4.0 |
| `Text::show` | 3.3 | 5.5 | +2.2 |
| `RgbaF32::to_srgba_u8` | — | 1.5 | +1.5 |
| `Cascade::is_within` (⅔ from `Scopes::resolve`) | — | 1.5 | +1.5 |
| `Block::show` | — | 1.4 | +1.4 |
| `Stop` slice compare (gradient dedup) | — | 1.3 | +1.3 |
| `Background::validate` | — | 1.2 | +1.2 |
| `AnimMap::animate` | 0.8 | 1.8 | +1.0 |
| `CascadeKey::new` | — | 0.9 | +0.9 |
| `Scopes::resolve` (self) | — | 0.7 | +0.7 |

The record pass is 72% of the frame: 1 244 nodes, 729 shapes and 491
chrome rows, about 1 860 instructions per node. `core::fmt` costs 8.5 µs
in both builds. That is the fixture's labels, so it is workload.

`RgbaF32::to_srgba_u8` comes from `Stop::new`, which encodes each stop's
colour every frame. `Background::validate` runs at the `background`
setter, as the project's validation rule requires. It costs about 2.4 ns
a call, so it is not a target.

## CPU design

### C1. Fold each node's rollup hashes at its `close_node`

`compute_rollups` walks the whole tree again in reverse pre-order after
the record pass. It reloads seven record columns per node and runs
`TreeItems` over each node's shapes and children. The per-node body only
needs the node's own columns, its direct shapes and its children's
finished subtree hashes. All of these are final at the node's
`close_node`, which runs in post-order:

- The only writes to a node's columns after its open are in
  `close_node`: its own `shape_span`, and its parent's `subtree_end`
  merge. The parent closes after all its children. (Checked: no other
  `*_mut()` writes to `records`, and nothing writes a shape after its
  push.)
- Post-order also has every descendant done before its ancestor, which
  is what the `container_text.remove_range` step needs.

**C1a was tried and failed.** The per-node body moved into a fold that
`close_node` called, with a test that held every column to the old
whole-tree pass. The columns matched, but `cached_cpu` rose from 139.5
to 162.0 µs (+16%, ABBA in both directions). A profile showed the fold
at about 32 µs inside `close_node` and an out-of-line `TreeItems::next`,
against 16.6 µs for the whole-tree walk: per node, the fold pays the
setup of seven column slices, a size check, and a `TreeItems` the
compiler no longer inlines. The whole-tree walk over columns that are
already in L2 is the efficient form, so the change was reverted.

**C1b is dropped too.** It streams the same hashes into `OpenFrame`
during the record pass, which puts more work at the same per-node call
sites that made C1a slower.

What is left of `compute_rollups` is its own arithmetic: four hashers
and the `TreeItems` interleave per node. A change there needs a profile
of that loop alone (`perf annotate` of `post_record`) before a design.

### C2. Resolve input scopes only when their inputs change

`Scopes::resolve` is a function of `focused`, the cascade, `closing` and
`closed`. Keep the inputs of the last resolve: the focused id, the
`CascadeKey` the cascade stores for its last build (an equal key means
the cascade skipped and its tables did not change), and a dirty bit that
`close` and `end_frame` set. When all are the same, keep `path`, `live`
and `outermost`. No new counter: the cascade already stores its key.
Estimate −1 to −1.5 µs. Test: one resolve per steady frame, and a fresh
one after a focus change, a cascade rebuild, a paint-only repair, a
`close` and the `end_frame` swap.

### C3. Compare gradient stops as words

**Decided:** crate-private. `Stop::as_u64` (`pub(crate) const fn`) packs
the offset into the low byte and the sRGB bytes above it, and the ramp's
equality compares those words. Estimate −0.5 to −1 µs. Test: two ramps
that differ in one byte of one stop are not equal, at every byte
position.

### C4. Shrink `IdEntry`

Measure `size_of::<IdEntry>()` first and pin it in `hot_struct_sizes.rs`.
`Recipe` holds a full `Ident` (16 bytes) and an `Option<WidgetId>` (16
bytes). It can be one `u64` key (the `Location` address or the salt id)
and one `u64` parent, with the kind and the presence of a parent in a
tag byte. A sentinel parent of 0 is **not** sound: `Configure::id`
accepts `WidgetId::default()`, which is 0, so a node can have the id 0.
Estimate −0.5 to −1 µs.

### C5. Investigate before designing

- `AnimMap::animate`, +1 µs: one hash probe per animated look per frame,
  also for looks at rest.
- `Text::show` +2.2 µs and `Block::show` +1.4 µs: the source of
  `Text::show` is short, so this is probably inlining that moved.
  `perf annotate` of both builds will show it.
- `CascadeKey::new`, 0.9 µs.
- `Stop::new` encodes each colour to sRGB bytes every frame (1.5 µs).
  `encode_byte` is already a table lookup, so only fewer calls would help.

## Measurement protocol

- **CPU items under 1.5 µs are below the build-to-build noise** (two
  builds of the same code differ by up to ±1.5 µs from code layout).
  Judge them by user instructions per frame (a `perf stat` difference of
  two `--profile-time` lengths, which is deterministic) and by their
  self time in a frame-pointer profile. Group C2, C3 and C4 into one ABBA
  wall-time comparison.
- **GPU items:** ABBA with 30 samples, plus fragment invocations and the
  per-kind times (`BatchKind::Shadows` separates the shadows).
- **Every step:** a full `frame` run with a `--note` row, and the visual
  suite for any shader or present change.

## Plan

1. **C2, C3, C4**, measured as one group.
2. **M1**: the alternating bench arm and the copy-out timestamp, then a
   decision on the per-frame choice of path.
3. **C5** profiles, written up here.
4. **G3**, only if a profile shows the edge terms as a large part of the
   remaining shadow cost.
5. README numbers from a full run, and `FRAME_BENCH_REGRESSION.md`
   updated with what each step recovered.

## Public API this plan touches

None. Everything left is crate-private.
