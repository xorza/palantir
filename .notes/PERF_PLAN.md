# Frame performance: design and plan

Measured on 2026-10-06 on the Ryzen 7 6800U / Radeon 680M
(`beelink-ser5-dev`), on the working tree over `77719e8a` (cutout tables
and in-step ids included). Background: `.notes/FRAME_BENCH_REGRESSION.md`.
All experiments ran on a copy of the tree with runtime switches. No
experiment code is in the tree.

## Result

- **GPU, shading:** the shadow shader is the whole regression of
  `scrolling_gpu` and `resizing_gpu`. G1 removes most of it.
- **GPU, copy-out:** the GPU arms now present the way the desktop does
  (`DirectAdaptive`, see `benches/AGENTS.md`). On that strategy a skip
  frame copies nothing, so `cached_gpu` is the CPU frame alone, and
  `partial_gpu` is the damage paint plus one full copy-out (M1).
- **CPU:** no single cause. Four internal changes recover an estimated
  4–7.5 µs, which is short of the old README by 3–7 µs on `cached_cpu`.
  C1b and the C5 investigations are where the rest must come from, and
  neither has a measured gain yet.

| arm | old README | now (desktop strategy) | estimate |
| --- | ---: | ---: | ---: |
| `cached_cpu` | 130 µs | 140.3 µs | 134–137 µs |
| `partial_cpu` | 145 µs | 155.2 µs | 148–151 µs |
| `scrolling_cpu` | 201 µs | 203.7 µs | 196–199 µs |
| `resizing_cpu` | 317 µs | 304.6 µs | 297–301 µs |
| `cached_gpu` | 1.12 ms | 0.131 ms | 0.125–0.128 ms |
| `partial_gpu` | 1.37 ms | 1.64 ms | ≈ 1.6 ms |
| `scrolling_gpu` | 4.25 ms | 4.86 ms | 3.5–3.9 ms |
| `resizing_gpu` | 5.38 ms | 6.00 ms | 4.7–5.1 ms |

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

### G1. Draw a shadow as a 3×3 grid of cells

WebRender splits a box shadow into segments and skips the ones it knows
are empty, and Skia draws a blurred rounded rect as a nine-patch. Each
segment evaluates only the terms that are not constant on it.

`vs_shadow` computes four grid lines per axis and draws the nine cells of
that grid as a triangle list (`pass.draw(0..54, range)` for
`RenderStep::Shadows`), with a flat `cell` varying.

**Grid lines.** On x: `X0` and `X3` are the bounds the quad has today.
`X1 = box.min.x + max(r_tl, r_bl) + reach` and
`X2 = box.max.x − max(r_tr, r_br) − reach`, where `box` and the radii are
the shadow's own (`fit_radii(spread_radius(..))`, as `fs_shadow` computes
them). y is the same with the top and bottom radii. Then every line is
clamped into `[X0, X3]`, and `X1` and `X2` are clamped to their midpoint
when they cross. So the lines are always in order, and the nine cells
tile the quad exactly, for every size.

**Watertight.** Every vertex of every cell is `(X[i], Y[j])`, one value
per grid line, computed once in the shader. Two cells that share an edge
share bit-identical vertices, so the rasterizer's fill rule shades each
pixel once. A collapsed cell has zero area and shades nothing. A pixel
shaded twice would blend twice, which is a visible bug, so a test must
show that it cannot happen (below).

**Forms per cell.** Each form is valid on its whole cell by construction,
with no condition on the box size:

| cell | form |
| --- | --- |
| corner (4) | today's full `blurred_box_coverage`, all terms and cutouts |
| edge (4) | the two `filter_cdf` across the edge. The pair along the edge is 1, and every cutout is 0, because the cell is more than `r + reach` from the perpendicular edges. |
| centre | drop shadow: the source clip, else `fill.a`. Inset shadow: 0. |

On the edge and centre cells, a term past `reach` is set to 0 or 1. This
moves a value by at most 3.2·10⁻⁵, about 1/120 of an 8-bit step
(**decided**: cut at `reach`). The corner cells keep today's exact tail.
The corner cutouts and the quad bounds already stop at `reach`.

**Centre skip.** For a drop shadow, the centre cell is not drawn when it
lies inside the hole: the source inset by its largest radius plus
`AA_HALF_WIDTH`. At a pixel centre there, the source SDF is at most
`−AA_HALF_WIDTH`, coverage is 1, and today's shader returns exactly
zero, so the skip is bit-exact. With a large offset or spread, the
centre cell can stick out of the hole. Then it is drawn with the centre
form, which is cheap. For an inset shadow, the centre form is 0, so the
centre is never drawn.

**σ below `CUTOUT_MIN_SIGMA`, and σ = 0.** The corner cells use the full
form, so they keep the outline integral for a small σ and the exact box
for σ = 0. With σ = 0, `reach = AA_HALF_WIDTH`, and the edge form is
`edge_coverage`, which is correct.

**`CutoutPlan`.** A corner reads its cutout only in its corner cell. So
the shaded area of a corner is its region inside the corner cell, and
the plan's estimate becomes more exact.

**Estimate.** The centre skip removes 2.7 M fragments (−0.5 to
−0.7 ms, and about 22 MB of traffic per full frame). The edge cells drop
from four `filter_cdf` and four cutout tests to two `filter_cdf`
(−0.4 to −0.6 ms). Total −0.9 to −1.3 ms on `scrolling_gpu` and
`resizing_gpu`.

**Tests.**

- A golden-free GPU A/B: render a sweep of shadows with the 3×3 grid and
  with today's single quad (an `internals` switch, as
  `disable_cutout_tables` does), and compare the images. The sweep:
  drop and inset; σ = 0, 0.2 (below `CUTOUT_MIN_SIGMA`), 2, 18; radius 0,
  4, 30; boxes larger than, equal to and smaller than `2·(r + reach)`;
  offset 0 and larger than `reach`; spread −4, 0, 6; a partial scissor
  across a cell edge; a rounded clip (stencil). Every channel must be
  within one level. The pixels of the skipped centre must be **equal**,
  not within one level.
- A double-blend test: a shadow with a colour whose double blend changes
  the 8-bit result at every pixel, over a known background. Any pixel
  shaded twice fails the A/B above, so this case goes into its sweep.
- The pipeline statistics (where the adapter has them) show the fragment
  count fall from 8.39 M toward 5.7 M on the first full frame of the
  fixture. This goes into the plan's measurement, not into the suite,
  because lavapipe has no pipeline statistics.
- `shader_body/tests.rs`: `fs` still reaches none of the shadow code, and
  the edge and centre forms reach no cutout function.

### G2. Measure shadows apart from quads

**Decided:** `BatchKind::Shadows = 4`, directly after `Quads`. `Text` to
`Icon` move up by one, and `COUNT` becomes 10.

The per-batch marker writes one timestamp at each change of kind. With
`Shadows` as a kind, the timed GPU arms write more timestamps inside the
pass. Measured on today's tree, the instrumentation costs ≤ 0.3 ms on
`scrolling_gpu` and nothing measurable on `cached_gpu`. So G2 goes in
first, and every later GPU step compares builds that both have it.

### G3. Only if G1 leaves the edges dominant: a baked edge profile

`filter_cdf(u, σ)` depends only on `u` and σ. One atlas row per distinct
σ, baked by the cutout pass, gives the edge profile with two texel loads
and a lerp. Do G3 only if a measurement after G1 shows the edge cells as
a large part of the shadow cost.

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

- **The shadow centre skip (G1)** removes about 22 MB of blend traffic
  per full frame.
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

**C1a.** Move the per-node body of `compute_rollups` into a function
that `close_node` calls for the closing node, with the same code, so the
hash streams stay the same. The one change is the cascade-static
hasher: it folds in post-order, not in reverse pre-order. A node hash is
compared only frame to frame, so any fixed order is correct.
`post_record` keeps the sizing (`reset_for`, `container_text.grow`),
which must then move to `pre_record` or grow per push. The columns are
hot at close for the leaves, which are most nodes. Estimate −2 to
−4 µs.

**C1b, only if C1a's profile shows the `TreeItems` walk as a large part
of what is left:** stream the shape and child folds into hashers in
`OpenFrame` at push and at child close. That is a second copy of the
hash logic, so it needs a clear measured gain.

**Tests.** Keep the old whole-tree `compute_rollups` in the test module
as the reference, and compare every rollup column and `container_text`
over the frame fixture and the random trees of the tree tests. The
cascade-static hash differs by design, so its test checks that it
changes exactly when an input the doc lists changes.

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
  per-kind times from G2.
- **Every step:** a full `frame` run with a `--note` row, and the visual
  suite for any shader or present change.

## Plan

1. **G2**.
2. **G1**, drop and inset together, because the grid is the same and the
   inset centre form is the simplest one.
3. **C1a**, with the reference test. Decide on C1b from its profile.
4. **C2, C3, C4**, measured as one group.
5. **M1**: the alternating bench arm and the copy-out timestamp, then a
   decision on the per-frame choice of path.
6. **C5** profiles, written up here.
7. **G3**, only if step 2 shows the edge cells as a large part of the
   remaining shadow cost.
8. README numbers from a full run, and `FRAME_BENCH_REGRESSION.md`
   updated with what each step recovered.

## Public API this plan touches

- `BatchKind::Shadows` (G2), approved. Regenerate `.notes/API_SURFACE.md`.

Everything else is crate-private.
