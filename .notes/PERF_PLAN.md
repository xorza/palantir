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
- **CPU:** no single cause. Of four internal changes, only the scope
  memo (C2) made the frame faster, by about 1.8 µs. Folding the rollups
  at `close_node` (C1), a word compare of gradient stops (C3) and a
  smaller `IdEntry` (C4) each made it slower and were reverted. So
  `cached_cpu` stays above the old README unless C5 finds more.

| arm | old README | now (desktop strategy) | estimate |
| --- | ---: | ---: | ---: |
| `cached_cpu` | 130 µs | 140.3 µs | ≈ 138.5 µs (C2 in) |
| `partial_cpu` | 145 µs | 155.2 µs | ≈ 153.5 µs (C2 in) |
| `scrolling_cpu` | 201 µs | 203.7 µs | ≈ 202 µs (C2 in) |
| `resizing_cpu` | 317 µs | 304.6 µs | ≈ 303 µs (C2 in) |
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

### The shadow grid, done

The shadow grid is in: a shadow skips the pixels its source hides, and
takes the two-term edge form or the full cover where the cutoff at
`reach` makes them exact. On the fixture it cut fragments from 8.39 M to
5.85 M, `scrolling_gpu` from 4.9 to 4.0 ms and `resizing_gpu` from 5.95
to 5.20 ms (ABBA, both directions). Shadows are still about 1.1 ms of a
full frame, against about 0.5 ms for every other quad.

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
- **The resync after a direct frame was tried, and did not pay.** The
  frame bench now has an `alternating` arm: a scroll step (a full
  repaint) and a counter tick (a small partial) in turn. A rule that sent
  a full frame through the backbuffer when the frame before it was a
  partial, so the next partial would stay cheap, moved
  `alternating_gpu` by −1.8% ± 4% and +3.1% ± 5% (ABBA): the copy it adds
  to the full frame costs about what the resync saves. Reverted; the arm
  stays.
- **The GPU arms depend on the GPU's power state.** With the
  `alternating` arm running first, `scrolling_gpu` and `resizing_gpu`
  read 3.3 and 3.9 ms, against 4.0 and 5.2 ms for the same code when
  `cached_gpu`, which leaves the GPU idle, ran before them. Compare GPU
  numbers only between runs of the same arm set, in the same order.
- **A timestamp around the copy-out** would separate the copy from the
  GPU wake-up. It needs a new public `GpuPassStats` reading, so it waits
  for a decision: see `PERF_PLAN_QUESTIONS.md`.

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

### C2–C4, measured one at a time

All three were built, tested and measured against the same base, one
binary each (`cached_cpu`, base re-run at 140.8 µs):

- **C2, the scope-resolve memo, is in:** about −1.8 µs. `Scopes::resolve`
  keeps its routing when the focus is the same, no withdrawal came, and
  the cascade kept the structure it was read from.
- **C3, the word compare of gradient stops, was reverted:** +1.5 µs. It
  converts all sixteen stops of the two ramps before it compares, which
  costs more than the field compare it replaced.
- **C4, a 48-byte `IdEntry`, was reverted:** +6.3 µs. At 64 bytes an entry
  is one cache line; at 48 most entries cross a line, on two tables that
  are written and read for every widget. `IdEntry` is now pinned at 64
  bytes in `hot_struct_sizes.rs`, and a test pins that a parent of id 0
  is a parent, which a later shrink with a sentinel would break.

### C5. What the investigations found

- `Text::show`, `Block::show` and `CascadeKey::new` have no hot line: a
  profile spreads each over many 0.1–0.3% items of the record path. This
  is inlining that moved, not new work, and there is no design to make.
- `AnimMap::animate` costs about 2.3 µs (1.6%). Each animated look pays
  two lookups per frame: the type map (`TypeId` and a downcast) and the
  row probe by `(id, slot)`. A concrete field for the `AnimatedLook` map,
  the crate's own animated type, would remove the first. Not done: a new
  item for a later round, with its own measurement.
- `Stop::new` encodes each stop's colour every frame (1.5 µs). The encode
  is already a table lookup; the calls are the fixture's own gradients.

### G3 measured and not done

With the edge form replaced by a constant, `scrolling_gpu` and
`resizing_gpu` fell only 0.13–0.19 ms. A baked edge profile would recover
part of that at most, so the edge terms are not a large part of the
shadow cost, and G3 is not done.

## Measurement protocol

- **CPU items under 1.5 µs are below the build-to-build noise** (two
  builds of the same code differ by up to ±1.5 µs from code layout).
  Measure each change in its own binary against the same base, and run
  the base again at the end for the drift. A group measurement hides a
  regression inside it: C2–C4 together read as +3.7 µs. `--profile-time`
  runs for a fixed time, not a fixed frame count, so its instruction
  counts do not compare per frame.
- **GPU items:** ABBA with 30 samples, plus fragment invocations and the
  per-kind times (`BatchKind::Shadows` separates the shadows).
- **Every step:** a full `frame` run with a `--note` row, and the visual
  suite for any shader or present change.

## Plan

1. **M1, copy-out timestamp**: blocked on `PERF_PLAN_QUESTIONS.md`.


## Public API this plan touches

Only M1's copy-out timing, if its question is answered with option 1: a
new `GpuPassStats::last_copy_out`.
