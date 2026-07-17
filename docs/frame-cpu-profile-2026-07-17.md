# Frame CPU Profiling Report

Date: 2026-07-17

## Executive summary

An x86-64-v3 build reduced frame CPU time by 14–19% without source changes,
but it was rejected as a global target because it would exclude older CPUs.
The workspace and Aperture-specific v3 configurations were removed.

The workload is instruction-bound. The largest remaining opportunities are
incremental cascade invalidation, eliminating a duplicate large theme clone,
and A/B-testing occlusion pruning. Widget ID/endpoint fusion and shape
lowering/hash fusion were implemented and benchmarked, but both regressed the
frame workload and were removed. Continuous resize allocations are
overwhelmingly caused by Cosmic Text buffer construction.

## Methodology

- CPU: AMD Ryzen 7 6800U
- CPU boost: disabled
- Benchmark: `frame`, with `internals`
- Workload: approximately 800 nodes and 500 text shapes
- Display: 3840×4800 physical pixels at 2× for the primary fixture
- Criterion: 3-second warm-up and 12-second measurement per arm
- Profiling: Linux `perf`, hardware counters, and precise AMD IBS samples
- Allocation profiling: DHAT through `alloc_free` and `alloc_resize`

The frame benchmark uses a deviceless CPU pipeline. Its CPU arms deliberately
force encode and compose even when cached damage would normally return
`Damage::Skip`. The cached result is therefore a comparable full-pipeline
workload, not the cost of a normal skipped presentation.

## Benchmark results

Criterion slope estimates:

| Frame arm | Workspace-root build | Intended v3 build | Time saved | Improvement |
|---|---:|---:|---:|---:|
| Cached | 371.98 µs | 302.79 µs | 69.20 µs | 18.60% |
| Partial | 338.73 µs | 290.84 µs | 47.89 µs | 14.14% |
| Resizing | 538.28 µs | 447.58 µs | 90.70 µs | 16.85% |
| Scrolling | 432.42 µs | 358.10 µs | 74.32 µs | 17.19% |

### Rejected x86-64-v3 experiment

A previous Aperture-specific `.cargo/config.toml` enabled
`target-cpu=x86-64-v3` on x86-64. Cargo did not load that member crate's
configuration when invoked from the workspace root, so the ordinary workspace
build lacked the statically enabled F16C path and used the slower packed-half
conversion implementation. The Aperture-specific configuration was later
removed along with the proposed workspace equivalent.

The target was not centralized at workspace scope because older x86-64 systems
must remain supported. A future SIMD optimization should keep the broad
baseline and use runtime-dispatched F16C routines structured so the hot loop
still vectorizes.

## Hardware-counter characterization

Post-v3 profiles measured:

- 3.05–3.30 instructions per cycle
- 0.18–0.24% branch misses
- 3.01–3.39% L1 data-cache misses
- 3.13–5.28% frontend-idle cycles

The pipeline is primarily instruction/retirement-bound, not branch- or
cache-bound. Reducing full-tree passes, hash-table operations, value copies,
and repeated record walks is more promising than data prefetching or branch
micro-optimization.

## Post-v3 hotspots

Precise IBS self-time samples:

| Arm | Largest self-time samples |
|---|---|
| Cached | compose 7.60%, node opening 7.63%, shapes 5.30%, button 5.17%, encode 4.85%, occlusion 4.22%, post-record 4.11%, `memmove` 4.09% |
| Partial | cascade 12.65%, node opening 7.76%, shapes 5.88%, `memmove` 5.41%, button 5.12%, post-record 4.27%, stack arrange 4.10% |
| Resizing | cascade 8.70%, measure 8.59%, node opening 5.10%, intrinsic compute 4.90%, `memmove` 4.20%, damage 3.70%, shapes 3.56% |
| Scrolling | cascade 10.44%, node opening 6.81%, compose 5.52%, damage 5.20%, `memmove` 4.51%, button 4.50%, shapes 4.36%, encode 4.12% |

## Ranked optimization opportunities

### 1. Rejected global x86-64-v3 target

Status: measured and rejected for compatibility.

This was worth 48–91 µs per frame in the measured workload and dominated every
source-level candidate, but it is not a valid project-wide optimization under
the supported CPU baseline.

The compatibility tradeoff is that x86-64-v3 requires AVX2, BMI1/2, FMA, and
F16C-class hardware. The configuration changes were removed from both the
workspace root and Aperture.

### 2. Make cascade invalidation incremental

Status: high potential, high architectural complexity.

[`cascade_fingerprint`](../src/ui/cascade/mod.rs) folds every root's complete
subtree hash, including paint content. A single footer text change therefore
invalidates the global fingerprint and reruns `CascadesEngine::run` across the
entire forest. Cascade self-time is approximately 37–39 µs in partial,
scrolling, and resizing frames.

Two viable directions are:

- Separate stable geometry/cascade-state validity from paint-content refresh,
  reusing transforms, clips, entries, and layout-dependent rectangles while
  refreshing changed paint rows and hashes.
- Add subtree-granular cascade invalidation, updating changed subtrees and the
  ancestor paint-bound rollup.

Simply weakening the existing global fingerprint is unsound because reused
paint arenas and cascade hashes would become stale.

### 3. Fuse widget ID resolution with endpoint reservation

Status: implemented, benchmarked, and rejected.

[`Ui::widget_id`](../src/ui/mod.rs) checks `SeenIds::curr` to resolve a unique
ID. The immediately following
[`Forest::open_node`](../src/forest/mod.rs) inserts that ID into the same map.
The public API already requires exactly one node opening after `widget_id`.

Use one `HashMap::entry` operation during ID resolution to detect collisions
and reserve the predicted `(layer, next_node)` endpoint. `Forest::open_node`
can then consume that reservation without another hash-table insertion.

The implementation used a single entry insertion at resolution and carried the
predicted endpoint into node opening. Popup bodies and modal roots required a
separate deferred reservation because they resolve before opening a side layer;
the popup also records its click-eater before its body.

Against the broad-target baseline, the refined implementation changed cached,
partial, resizing, and scrolling by +2.45%, +0.43%, +0.77%, and +0.99%
respectively. It therefore failed the optimization criterion and was removed.
The scalar reservation and cross-layer fallback cost more than the eliminated
map probe.

### 4. Remove the duplicate large theme clone

Status: small, concrete, low-to-moderate risk.

[`resolve_look`](../src/widgets/theme/mod.rs) clones the selected `WidgetLook`
to end the immutable theme borrow before mutably borrowing `Ui`.
`WidgetLook::animate(&self)` then clones the owned look's large `Background`
again.

Add an owned/by-value animation path so the already cloned `WidgetLook` moves
its background into `AnimatedLook`. This targets part of the persistent 4–5%
`memmove` cost while preserving the necessary first clone.

### 5. Fuse shape lowering and hashing

Status: implemented, benchmarked, and rejected.

[`Shapes::add`](../src/forest/shapes/mod.rs) lowers a `Shape` into a
`ShapeRecord`, then `compute_record_hash` rereads the complete record through a
second variant match. Have each lowering arm return a named result containing
both the record and its hash, reusing values already loaded during lowering.

All eleven variant schedules were cross-checked exactly against the original
record-walking hash. Despite eliminating the second variant match, the fused
implementation added approximately 1.6–3.0% on top of the ID-only binary,
consistent with code-size and register-pressure costs. The compiler already
handles the compact centralized matcher better than the manually expanded
variant paths. The change was removed.

## Rejected fusion benchmark

Criterion slope estimates used a generic x86-64 build, CPU boost disabled,
one pinned core, 1-second warm-up, 5-second measurement, and 50 samples:

| Frame arm | Baseline | ID fusion | ID + shape fusion |
|---|---:|---:|---:|
| Cached | 434.90 µs | 445.89 µs (+2.45%) | 453.18 µs (+4.05%) |
| Partial | 395.11 µs | 397.06 µs (+0.43%) | 408.87 µs (+3.38%) |
| Resizing | 594.81 µs | 599.15 µs (+0.77%) | 610.67 µs (+2.14%) |
| Scrolling | 504.99 µs | 510.36 µs (+0.99%) | 520.93 µs (+3.04%) |

No fusion code remains in the worktree.

### 6. A/B-test CPU occlusion pruning

Status: experiment required.

[`OcclusionPruner::prune`](../src/renderer/frontend/composer/occlusion.rs)
costs 4.22%, approximately 13 µs, in cached full-paint frames. That CPU work may
still be profitable if it removes enough GPU overdraw.

Add benchmark-only instrumentation to compare:

- CPU compose time with pruning enabled and disabled
- emitted and removed quad counts
- representative full GPU frame time

If few quads are removed, gate pruning on group size or opaque-quad count.

### 7. Recycle Cosmic Text buffers during continuous resize

Status: allocation optimization; CPU benefit must be remeasured.

Allocation results:

| Workload | Blocks/frame | Bytes/frame |
|---|---:|---:|
| Cached steady state | 0 | 0 |
| Four-size resize rotation | 0.01 | 1,464 |
| Unique-width continuous drag | 343.38 | 182,981 |

DHAT attributes the continuous-resize growth overwhelmingly to
`cosmic_text::Buffer` construction and its glyph/layout vectors. Across the
complete DHAT run, stacks rooted through Aperture's text path accounted for
43.4 MB and 78,506 blocks. Direct measure-cache stacks accounted for 1.5 MB,
and damage stacks for 0.18 MB.

Keep a bounded pool of evicted Cosmic Text buffers and reset/reuse their
internal vector capacity for new wrap widths. This preserves the cache's
bounded-width policy while avoiding repeated glyph-vector allocation during a
window-edge drag.

### 8. Precompute stable formatted labels

Status: fixture/application optimization.

`core::fmt::write` accounts for approximately 1.2–1.9% after v3 because the
fixture formats indexed labels every frame. Stable labels such as action,
sidebar, tag, and username strings can be precomputed or stored in application
state. This is not a framework-level priority.

## Suggested implementation order

1. Implement the owned theme-animation path.
2. Prototype incremental cascade reuse against partial and scrolling arms.
3. Run the occlusion-pruning CPU/GPU A/B experiment.
4. Prototype a bounded Cosmic Text buffer recycle pool and validate continuous
   resize allocation and CPU time.

Each source change should be measured independently against all four frame arms
and checked with `alloc_free` so improvements do not compromise steady-state
allocation behavior.

## Profiling artifacts

Raw `perf`, IBS, Criterion, and DHAT outputs are stored in the workspace-local
`.tmp/` directory. They are intentionally not part of the repository.
