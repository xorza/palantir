# Renderer simplification and optimization review

Reviewed 2026-07-25 at Palantir commit `72f030a3`.

## Scope and conclusion

This review covers `renderer/` production code, shaders, tests, and existing
benchmarks. It is a static audit, not a performance claim: no recommendation
below should be credited with a speedup until its proposed benchmark moves.

The renderer's overall architecture is sound. Its single-pass
encode → compose → submit flow, retained scratch storage, typed instance
buffers, and paint-order-aware batching all encode constraints that a broad
"simplification" would likely lose. The best opportunities are narrower:

1. Avoid nearest-filter setup in the common bilinear image shader path.
2. Eliminate redundant render-pass state commands.
3. Amortize full-map text-cache maintenance.
4. Coalesce consecutive image instances that use the same texture.
5. Replace two small repeated-work algorithms with monotonic or unique work.

There is no evidence here that the renderer needs a rewrite or a new
abstraction layer. The first six findings are bounded changes. The final two
are measurement-gated design options and should not be implemented merely to
reduce source size.

## Priority summary

| Priority | Change | Expected benefit | Risk | Code effect |
| --- | --- | --- | --- | --- |
| P1 | Skip unused nearest-filter setup for bilinear images | GPU fragment work | Low–medium | Neutral |
| P1 | Deduplicate scissor state commands | CPU command encoding | Low | Small reduction |
| P1 | Amortize encoded text-cache sweeps | CPU time in large text caches | Low | Small increase |
| P1 | Coalesce adjacent same-texture image draws | Bind/draw command count | Low | Small increase |
| P2 | Bake gradient LUTs with a monotonic stop cursor | CPU work during gradient creation | Low | Reduction |
| P2 | Insert each text-grid spill entry once | Pathological compose time and memory | Low | Neutral |
| P3 | Partition retained render targets by owner | Multi-window GpuView maintenance | Medium | Structural increase |
| P3 | Avoid uploading unused mesh payload ranges | Partial-damage mesh upload volume | High | Structural increase |

P1 means “benchmark and implement first”, not “assume the result”.

## Findings

### P1 — Keep nearest-filter setup out of the common bilinear path

The image fragment shader currently computes texture dimensions, UV
derivatives, two footprint dot products, and a minification/magnification
choice for every fragment:

- [`backend/image_pipeline/image.wgsl`](backend/image_pipeline/image.wgsl#L65-L94)
- [`frontend/payload.rs`](frontend/payload.rs#L266-L269) documents zero flags as
  the common case, including GpuView rendering.

Only nearest filtering consumes the footprint result. With zero flags, all of
that setup is discarded before the ordinary `textureSample`.

Recommended shape:

- Read the nearest-related flag bits first.
- Keep `dpdx`/`dpdy` outside non-uniform control flow to satisfy WGSL derivative
  uniformity rules.
- Put `textureDimensions`, footprint conversion, dot products, and filter
  selection behind the nearest-flags branch.
- Consider a separate bilinear shader/pipeline variant only if profiling shows
  the unconditional derivative operations themselves are material. A new
  variant adds pipeline state and batching complexity, so it is not the first
  move.

Verification:

- Add visual cases for bilinear, min-nearest, mag-nearest, both-nearest, and
  tiled combinations at fractional scales.
- Add an image-heavy GPU benchmark with zero flags and a nearest-filter control.
- Run the shader validation and renderer visual suite.

### P1 — Deduplicate scissor state commands at one ownership point

The scheduler has several sites that establish a scissor:

- The non-stencil group path establishes one before group drawing in
  [`backend/schedule.rs`](backend/schedule.rs#L254-L269).
- The higher-kind continuation establishes one again after the text drain in
  [`backend/schedule.rs`](backend/schedule.rs#L494-L514).
- `StencilTracker::establish` and `clear_active` also emit scissor changes in
  [`backend/schedule.rs`](backend/schedule.rs#L327-L360).

For a group containing quads and a higher-kind primitive but no text, the
logical sequence can include:

```text
SetScissor → Quads → SetScissor(same value) → Images/Curves/Meshes
```

Pipeline binding already avoids redundant `set_pipeline` calls in
[`backend/mod.rs`](backend/mod.rs#L790-L842), but scissor commands are replayed
as scheduled.

Recommended change:

- Give schedule construction one `last_scissor` state and route every scissor
  emission through a helper that only pushes a changed value.
- Keep the first scissor in a render pass mandatory.
- Do not infer scissor state independently in text, stencil, and higher-kind
  branches.

This is both an optimization and a simplification: state transition ownership
moves to one place, while branch-specific code only requests the desired state.

Verification:

- Add exact schedule tests for:
  - quads followed by images with no text;
  - text followed by higher-kind drawing;
  - entering, retaining, replacing, and leaving a stencil chain;
  - equal and unequal scissors on adjacent groups.
- Assert both draw order and the number of emitted `SetScissor` steps.
- Benchmark CPU render-pass recording for many mixed groups. Command count is a
  useful secondary metric, but wall time is the decision metric.

### P1 — Amortize full-map maintenance in the encoded text cache

`EncodedCache::sweep` retains over the whole map, sums live spans, and may
compact the encoded buffer:

- [`backend/text/encode.rs`](backend/text/encode.rs#L121-L142)
- It is called by `TextEncoder::end_frame` for every frame that submits text in
  [`backend/text/encode.rs`](backend/text/encode.rs#L231-L237).

Entries are retained for 120 text frames. Most calls therefore scan the entire
map without expiring anything. The glyph atlas already demonstrates the
appropriate pattern by gating stale unallocated-entry cleanup to a cadence in
[`backend/text/atlas.rs`](backend/text/atlas.rs#L530-L555).

Recommended change:

- Run encoded-cache expiry on a fixed cadence such as every 32 text frames.
- Accept a bounded extra lifetime of at most one cadence, or subtract the
  cadence from the retention threshold if the exact upper bound matters.
- Calculate live length and consider compaction only on sweep frames.
- When `try_emit_cached` finds a stale atlas generation, remove that cache row
  after releasing the map borrow instead of paying the same failed lookup until
  the next sweep.

Do not introduce a second cache or a background maintenance path. A cadence
gate preserves the existing design and makes its cost proportional to actual
maintenance frequency.

Verification:

- Extend the text-atlas benchmark with a large, stable key set and with churn.
- Assert a hard bound on stale row lifetime and encoded storage growth.
- Preserve exact emitted glyph instances before and after compaction.

### P1 — Coalesce adjacent image draws that bind the same texture

An image batch currently loops over each image ID and issues one draw:

- [`backend/mod.rs`](backend/mod.rs#L942-L951)
- [`backend/image_pipeline/mod.rs`](backend/image_pipeline/mod.rs#L210-L220)

This is necessary when textures differ, but not when consecutive instances use
the same `TextureId`. Paint order permits adjacent equal IDs to be represented
as one instance range without reordering anything.

Recommended change:

- Run-length encode only adjacent equal texture IDs inside each existing image
  batch.
- Bind once and draw `start..end` for each run.
- Do not globally sort or group by texture; that would break painter's order.
- Keep the instance buffer unchanged.

This should be implemented only after adding a benchmark that shows the
renderer has meaningful same-texture runs. Repeated icons and repeated views
are the likely wins; a workload of alternating IDs is the required control and
should remain effectively unchanged.

Verification:

- Assert identical image order for repeated and alternating texture patterns.
- Count bind/draw calls in a test seam or benchmark instrumentation.
- Benchmark one long same-ID run, short mixed runs, and all-unique IDs.

### P2 — Advance through gradient stops once per LUT

Gradient LUT baking samples 256 increasing values of `t`, but `lerp_at` starts
its stop search at index 1 for every texel:

- [`gradient_atlas/bake.rs`](gradient_atlas/bake.rs#L35-L71)

The stops are sorted and the sampled `t` values are monotonic, so the upper-stop
cursor can only move forward. Carry it across the LUT loop. This changes stop
comparisons from `O(LUT_SIZE × stop_count)` to
`O(LUT_SIZE + stop_count)` and can remove the repeated-search helper.

The maximum stop count is small, so this is a P2 cleanup rather than a presumed
large win. It is attractive because the faster algorithm is also simpler.

Verification:

- Compare all 256 output texels exactly against the current algorithm for:
  - two stops;
  - maximum stops;
  - equal neighboring offsets;
  - stops at 0 and 1;
  - all supported color spaces.
- Extend the existing gradient benchmark with maximum-stop LUT creation.

### P2 — Add a text rectangle to the spill list only once

`TextRectGrid::push` visits every overlapped tile. When a tile is full, it
pushes the rectangle index into the shared spill list:

- [`frontend/composer/text_grid.rs`](frontend/composer/text_grid.rs#L143-L160)

A large rectangle crossing several saturated tiles can therefore appear in the
spill list several times. Every query then checks every duplicate:

- [`frontend/composer/text_grid.rs`](frontend/composer/text_grid.rs#L186-L201)

Recommended change:

- Record a local `spilled` boolean while visiting tiles.
- Append the rectangle index to the shared spill list once after the tile loop.
- Preserve all per-tile insertion behavior and query ordering.

This removes duplicate retained storage and repeated intersection tests in the
pathological workload the spill mechanism is intended to handle.

Verification:

- Saturate several neighboring tiles, then insert one rectangle spanning all
  of them.
- Assert the same query result set and exactly one spill entry for that
  rectangle.
- Add a compose benchmark with many large overlapping text bounds.

### P3 — Partition retained render targets by owner only if multi-window data warrants it

After painting one owner's targets, render-target retention scans the global
entry map:

- [`backend/image_pipeline/render_target.rs`](backend/image_pipeline/render_target.rs#L81-L87)
- Owner retirement scans it again in
  [`backend/image_pipeline/render_target.rs`](backend/image_pipeline/render_target.rs#L96-L103).

With one window or few GpuViews this is appropriately simple. With many shared
backend windows, each submission pays for unrelated owners' targets.

Before changing it, benchmark target maintenance across multiple owners. If it
is material, partition entries by `RenderOwnerId` so paint-time retention only
touches the active owner and retirement can remove an owner directly. Preserve
global texture-ID uniqueness and registry removal behavior.

This is deliberately P3: nested ownership storage adds structure and may cost
more than the scan it removes in the normal case.

### P3 — Measure unused mesh uploads before designing a retained mesh cache

If any composed mesh instance survives, submission uploads the complete
recorded vertex and index payloads:

- [`backend/mod.rs`](backend/mod.rs#L491-L497)
- [`backend/mesh_pipeline.rs`](backend/mesh_pipeline.rs#L79-L94)

Partial damage or clipping can leave one visible mesh instance while most
recorded mesh geometry is unused. That can turn a small paint into a full-scene
CPU-to-GPU mesh upload.

First add a benchmark with a large mesh scene and a small damaged region. If
upload volume is material, evaluate range-aware uploads. Do not compact
vertices or indices without rewriting both absolute index spans and
`base_vertex` consistently.

A retained content-hash GPU mesh cache is not the first answer. It adds
invalidation, memory policy, and lookup costs that conflict with the current
straight-line submission design. The measured workload must justify that
complexity.

## Code-reduction assessment

### Reductions worth making

- Centralize scissor emission and delete branch-local duplicate state
  establishment.
- Fold gradient interpolation into a monotonic bake cursor if that removes the
  repeated-search helper cleanly.
- Remove invalid encoded-cache rows at the point they are detected.
- Keep run coalescing local to image submission rather than adding a generic
  batching abstraction.

### Large reductions that are likely regressions

- Do not restore a retained command buffer or compose cache. The current
  single-pass path and retained scratch buffers avoid the invalidation and
  allocation costs those layers introduce.
- Do not merge typed render-buffer columns into one boxed or enum command list.
  The existing layout keeps payloads compact and submission loops specialized.
- Do not unify image, curve, mesh, and text pipelines merely because their
  upload code looks similar. Their bind groups, instance formats, shader state,
  and ordering constraints differ at the important boundaries.
- Do not sort higher-kind primitives or images globally for fewer state changes.
  Existing batching preserves painter's order.
- Do not replace retained scratch vectors with locally collected iterators or
  temporary vectors. Source may shorten while steady-state allocations return.
- Do not reduce the large renderer test corpus by deleting cases. Consolidate
  only cases that share a fixture and still report the failing axis clearly.

`frontend/composer/mod.rs` and `backend/mod.rs` are long, but their main
orchestration is cohesive and their inherent implementations are intentionally
located with their types. Splitting them solely by line count would increase
navigation cost without reducing runtime or conceptual complexity. Extract a
module only when one of the changes above produces an independently owned
algorithm or state object.

## Missing benchmark coverage

The existing benchmarks cover curve workloads, higher-kind overlap, and
several text-atlas scenarios. The review needs the following additions before
optimization claims can be accepted:

| Benchmark | Primary metric | Control |
| --- | --- | --- |
| Zero-flag image fragments | GPU time | Nearest-filter flags |
| Same-texture image runs | CPU record time and draw count | Alternating IDs |
| Mixed-group scheduling | CPU record time and scissor count | Current schedule |
| Large stable encoded cache | Text encode time | Cache churn |
| Maximum-stop LUT creation | LUT bake time | Two-stop gradient |
| Saturated text grid | Compose time and spill length | Unsaturated grid |
| Multi-owner GpuViews | Target-maintenance time | Single owner |
| Small damage in a large mesh scene | Bytes uploaded and submit time | Full damage |

Measure representative release builds. Command counts and uploaded bytes
explain a result but do not replace elapsed CPU/GPU measurements.

## Suggested implementation order

1. Add the missing microbenchmarks and record the baseline.
2. Implement the gradient cursor and unique text-grid spill entry; both are
   local and easy to prove exactly.
3. Centralize scissor deduplication and lock the schedule down with exact step
   tests.
4. Gate encoded-cache sweeps and verify memory bounds under churn.
5. Optimize the image shader and run the full visual suite.
6. Coalesce adjacent image runs if the benchmark shows enough repetition.
7. Revisit owner-partitioned targets and mesh upload ranges only if their
   dedicated measurements are significant.

## Audit notes

- Renderer inventory: 52 Rust files and 5 WGSL files, approximately 16.7k Rust
  lines and 1.1k shader lines including tests and documentation.
- The diagnostic hot-structure size test passed. Notable current sizes are
  `DrawRectPayload` 60 bytes, `DrawText` 56, `DrawImage` 56, `Quad` 60,
  `GlyphInstance` 20, and `TextRun` 64.
- No runtime benchmark result is claimed by this document.
- No production source was changed as part of the review.
